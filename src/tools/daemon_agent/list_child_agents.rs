use crate::agent::{AgentGroup, AgentId};
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use itertools::Itertools;
use log::info;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Serialize;
use serde_json::json;

#[derive(Clone)]
pub struct ListChildAgentsTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
pub struct Args {}

#[derive(Debug, Clone, Serialize)]
struct ChildAgentInfo<'a> {
    agent_id: &'a AgentId,
    agent_group: &'a AgentGroup,
    desc: &'a str,
}

#[allow(async_fn_in_trait)]
impl Tool for ListChildAgentsTool {
    const NAME: &'static str = "list-daemon-agents";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List all active child daemon agents and their configurations".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        info!("Listing all child daemon agents");
        let parent_ctx = self.ctx.parent_agent.context();
        let children = parent_ctx.children.read().await;

        if children.is_empty() {
            return Ok(ToolCallRsult {
                success: true,
                output: "No active child daemon agents found.".to_string(),
                error: None,
            });
        }

        let infos = children
            .iter()
            .map(|(_, child)| ChildAgentInfo {
                agent_id: child.id(),
                agent_group: child.agent_group(),
                desc: child.description(),
            })
            .sorted_by_key(|it| it.agent_id)
            .collect_vec();
        let mut md = String::from("### Active Child Daemon Agents\n\n");
        md.push_str("| Agent ID | Agent Group | System Prompt |\n");
        md.push_str("| --- | --- | --- |\n");
        for info in &infos {
            md.push_str(&format!(
                "| `{}` | `{}` | {} |\n",
                info.agent_id, info.agent_group, info.desc
            ));
        }
        Ok(ToolCallRsult {
            success: true,
            output: md,
            error: None,
        })
    }
}
