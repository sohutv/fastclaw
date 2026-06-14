use crate::agent::llm_agent::LlmAgent;
use crate::agent::{Agent, AgentRequest, ToolFilter};
use crate::channels::{ChannelMessage, SessionId};
use crate::model_provider::ModelProvider;
use crate::tools::ToolContext;
use itertools::Itertools;
use rig::agent::Agent as RigAgent;
use rig::client::CompletionClient;
use rig::providers::openai::responses_api::ReasoningEffort;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    pub(super) async fn create_agent<TF>(
        self: &Arc<Self>,
        session_id: SessionId,
        addi_system_prompt: Option<&str>,
        channel_message_sender: Sender<crate::Result<ChannelMessage>>,
        tool_filter: TF,
        agent_request: Option<&AgentRequest>,
    ) -> crate::Result<RigAgent<C::CompletionModel>>
    where
        P: ModelProvider<Client = C>,
        TF: Into<ToolFilter>,
    {
        let model_client = &self.model_provider.completion_client()?;
        let preamble = if let Some(dst) = &self.ctx.system_prompt {
            &*dst
        } else {
            &*super::super::prompt::PromptSection::Identity
                .build(&self.ctx)
                .await?
        };
        let mut builder = model_client
            .agent(&*self.model_name)
            .preamble(preamble)
            .append_preamble(&format!(
                r#"
# MetaData
- **Your AgentId**: {}
            "#,
                &self.id
            ))
            .append_preamble(addi_system_prompt.unwrap_or_default())
            .tools({
                let filter = self
                    .agent_settings
                    .tool_filter
                    .clone()
                    .map(|it| ToolFilter::from(it))
                    .unwrap_or_default()
                    .and(tool_filter.into());
                crate::tools::FunctionTool::required_tools(ToolContext {
                    session_id,
                    config: self.ctx.config,
                    parent_agent: Arc::clone(&self) as Arc<dyn Agent>,
                    channel_message_sender,
                    mcp_registry: self.ctx.mcp_registry,
                    agent_request: agent_request.map(|it| it.clone()),
                })
                .await?
                .into_iter()
                .flat_map(|tool| filter.filter(tool))
                .collect_vec()
            })
            .temperature(self.agent_settings.temperature)
            .default_max_turns(self.agent_settings.max_turns)
            .max_tokens(
                self.agent_settings
                    .max_tokens
                    .unwrap_or(self.model_settings.max_tokens),
            )
            .additional_params(AdditionalParams::from( &**self).jsonify()?);
        if let Some(s) = &self.agent_settings.output_schema {
            builder = builder.output_schema_raw(s.clone());
        }
        let agent = builder.build();
        Ok(agent)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdditionalParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<ReasoningEffort>,
}

impl AdditionalParams {
    fn jsonify(self) -> crate::Result<serde_json::Value> {
        Ok(serde_json::to_value(self)?)
    }
}
impl<C, P> From<&LlmAgent<C, P>> for AdditionalParams
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    fn from(agent: &LlmAgent<C, P>) -> Self {
        Self {
            reasoning_effort: {
                if agent.model_settings.reasoning {
                    Some(agent.agent_settings.reasoning_effort.clone())
                } else {
                    None
                }
            },
        }
    }
}
