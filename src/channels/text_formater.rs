use crate::agent::HistoryCompactResult;
use crate::channels::{AgentRespType, SessionId};
use derive_more::From;
use itertools::Itertools;
use rig::completion::{AssistantContent, Usage};
use rig::message::{
    Message, MimeType, Reasoning, ReasoningContent, ToolCall, ToolFunction, ToolResult,
    ToolResultContent, UserContent,
};
use serde::ser::Error;
use std::fmt::{Display, Formatter};
use std::mem;

pub(super) fn format_tool_call(
    session_id: &SessionId,
    ToolCall {
        call_id,
        function: ToolFunction { name, arguments },
        ..
    }: &ToolCall,
) -> Option<(String, AgentRespType)> {
    let true = session_id.settings().show_toolcall else {
        return None;
    };
    let text = format!(
        r#"
### 工具调用: {name}...
- {}
```
{}
```json
"#,
        call_id.as_deref().unwrap_or("<unknown-id>"),
        serde_json::to_string_pretty(arguments)
            .unwrap_or_else(|err| format!("<serializing arguments err: {}>", err))
    );
    Some((text, AgentRespType::ToolCall))
}

pub(super) fn format_tool_result(
    session_id: &SessionId,
    ToolResult {
        call_id, content, ..
    }: &ToolResult,
) -> Option<(String, AgentRespType)> {
    let true = session_id.settings().show_toolcall else {
        return None;
    };
    let text = content
        .iter()
        .flat_map(|content| match content {
            ToolResultContent::Text(text) => vec![text.to_string()],
            ToolResultContent::Image(image) => vec![
                image
                    .clone()
                    .try_into_url()
                    .unwrap_or_else(|image| image.to_string()),
            ],
        })
        .collect::<Vec<_>>()
        .join("\n");
    let text = format!(
        r#"
### 工具结果...
- {}
```
{}
```json
"#,
        call_id.as_deref().unwrap_or("<unknown-id>"),
        text
    );
    Some((text, AgentRespType::ToolResult))
}

pub(super) fn format_reasoning(_: &SessionId, buff: &mut Vec<String>) -> (String, AgentRespType) {
    let content = mem::replace(buff, vec![]).join("");
    let text = format!(
        r#"
### 我的想法..
{content}
"#
    );
    (text, AgentRespType::Reasoning)
}

#[derive(Debug, Clone, From)]
pub enum FormatedMessage {
    Markdown(String),
    Json(serde_json::Value),
}

impl Display for FormatedMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatedMessage::Markdown(text) => write!(f, "{}", text),
            FormatedMessage::Json(json) => {
                let text = serde_json::to_string_pretty(json)
                    .map_err(|err| Error::custom(format!("{err}")))?;
                write!(f, "{}", text)
            }
        }
    }
}

pub(super) fn format_message(
    session_id: &SessionId,
    output_schema: bool,
    usage: &Usage,
    buff: &mut Vec<String>,
) -> crate::Result<(FormatedMessage, AgentRespType)> {
    let text = mem::replace(buff, vec![]).join("");
    let formated = if output_schema {
        let json_value = serde_json::from_str::<serde_json::Value>(&text)?;
        let json = serde_json::json!({
            "data": json_value,
            "token_usage": usage,
        });
        FormatedMessage::Json(json)
    } else {
        let text = if session_id.settings().show_token_usage {
            format!(
                r#"
{}

*<<Tokens:{}↑{}↓{}>>*
"#,
                text, usage.total_tokens, usage.input_tokens, usage.output_tokens
            )
        } else {
            text
        };
        FormatedMessage::Markdown(text)
    };
    Ok((formated, AgentRespType::Content))
}

pub(super) fn format_history_compact(
    _: &SessionId,
    result: &HistoryCompactResult,
) -> (String, AgentRespType) {
    match result {
        HistoryCompactResult::Ok(val) => (
            format!(
                r#"
### 压缩上下文完成
- 压缩前 **{}** Tokens
- 压缩后 **{}** Tokens
- 压缩率 **{:.2}%**
"#,
                val.before().total_tokens,
                val.current().total_tokens,
                val.compact_ratio(),
            ),
            AgentRespType::HistoryCompactOk,
        ),
        HistoryCompactResult::Err(err_msg) => (
            format!(
                r#"
### 压缩上下文失败
{}
"#,
                err_msg.to_string()
            ),
            AgentRespType::HistoryCompactErr,
        ),
        HistoryCompactResult::Ignore(msg) => (
            format!(
                r#"
### 压缩请求被忽略
{msg}
"#
            ),
            AgentRespType::HistoryCompactIgnore,
        ),
    }
}

pub(super) fn extract_reasoning(session_id: &SessionId, reasoning: &Reasoning) -> Vec<String> {
    if session_id.settings().show_reasoning {
        reasoning
            .content
            .iter()
            .filter_map(|content| match content {
                ReasoningContent::Text { text, .. } => Some(text.to_string()),
                ReasoningContent::Encrypted(_) => Some("<encrypted data>".to_string()),
                ReasoningContent::Redacted { data } => Some(data.to_string()),
                ReasoningContent::Summary(summary) => Some(summary.to_string()),
                _ => None,
            })
            .collect_vec()
    } else {
        vec![]
    }
}

pub(super) fn extract_message(session_id: &SessionId, message: &Message) -> Vec<String> {
    match message {
        Message::User { content } => content
            .iter()
            .flat_map(|content| match content {
                UserContent::Text(text) => vec![text.to_string()],
                UserContent::ToolResult(toolcall) => format_tool_result(session_id, toolcall)
                    .into_iter()
                    .map(|it| it.0)
                    .collect_vec(),
                UserContent::Image(image) => vec![
                    image
                        .clone()
                        .try_into_url()
                        .unwrap_or_else(|image| image.to_string()),
                ],
                UserContent::Audio(audio) => vec![
                    audio
                        .media_type
                        .as_ref()
                        .map(|it| it.to_mime_type())
                        .unwrap_or_default()
                        .to_string(),
                ],
                UserContent::Video(video) => vec![
                    video
                        .media_type
                        .as_ref()
                        .map(|it| it.to_mime_type())
                        .unwrap_or_default()
                        .to_string(),
                ],
                UserContent::Document(doc) => vec![
                    doc.media_type
                        .as_ref()
                        .map(|it| it.to_mime_type())
                        .unwrap_or_default()
                        .to_string(),
                ],
            })
            .collect_vec(),
        Message::Assistant { content, .. } => content
            .iter()
            .flat_map(|content| match content {
                AssistantContent::Text(text) => vec![text.to_string()],
                AssistantContent::ToolCall(toolcall) => format_tool_call(session_id, toolcall)
                    .into_iter()
                    .map(|it| it.0)
                    .collect_vec(),
                AssistantContent::Reasoning(reasoning) => extract_reasoning(session_id, reasoning),
                AssistantContent::Image(image) => vec![
                    image
                        .clone()
                        .try_into_url()
                        .unwrap_or_else(|image| image.to_string()),
                ],
            })
            .collect_vec(),
        Message::System { .. } => vec![],
    }
}
