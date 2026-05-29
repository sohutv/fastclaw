use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};
use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpToolConfig {
    #[serde(rename = "stdio")]
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    #[serde(rename = "sse")]
    Sse {
        url: String,
    },
}

#[derive(Clone)]
pub struct McpHandler {
    pub server_name: String,
}

impl rmcp::handler::client::ClientHandler for McpHandler {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        rmcp::model::ClientInfo::new(
            rmcp::model::ClientCapabilities::default(),
            rmcp::model::Implementation::new("fastclaw", "0.2.4"),
        )
    }

    async fn on_tool_list_changed(
        &self,
        context: rmcp::service::NotificationContext<rmcp::service::RoleClient>,
    ) {
        log::info!("MCP server '{}' notified tool list changed, re-fetching...", self.server_name);
        match context.peer.list_all_tools().await {
            Ok(tools) => {
                let mcp_tools = tools
                    .into_iter()
                    .map(|t| rig::tool::rmcp::McpTool::from_mcp_server(t, context.peer.clone()))
                    .collect::<Vec<_>>();
                
                let reg = registry();
                let mut guard = reg.tools.write().unwrap();
                guard.insert(self.server_name.clone(), mcp_tools);
                log::info!(
                    "MCP server '{}' updated with {} tools.",
                    self.server_name,
                    guard.get(&self.server_name).map(|v| v.len()).unwrap_or(0)
                );
            }
            Err(e) => {
                log::error!("Failed to re-fetch tools from MCP server '{}': {}", self.server_name, e);
            }
        }
    }
}

pub struct McpRegistry {
    pub tools: Arc<RwLock<BTreeMap<String, Vec<rig::tool::rmcp::McpTool>>>>,
    pub services: Arc<RwLock<Vec<tokio::task::JoinHandle<()>>>>,
}

static REGISTRY: OnceLock<McpRegistry> = OnceLock::new();

pub fn registry() -> &'static McpRegistry {
    REGISTRY.get_or_init(|| McpRegistry {
        tools: Arc::new(RwLock::new(BTreeMap::new())),
        services: Arc::new(RwLock::new(Vec::new())),
    })
}

pub async fn init_mcp_tools(config: &'static Config) -> crate::Result<()> {
    let mcp_tools_config = match &config.mcp_tools {
        Some(m) => m,
        None => return Ok(()),
    };

    let reg = registry();

    for (name, tool_config) in mcp_tools_config {
        log::info!("Initializing MCP tool/server: {}", name);
        let handler = McpHandler { server_name: name.clone() };

        match tool_config {
            McpToolConfig::Stdio { command, args, env } => {
                let mut cmd = match rmcp::transport::child_process::which_command(command) {
                    Ok(c) => c,
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to find executable '{}': {}", command, e));
                    }
                };
                cmd.args(args);
                for (k, v) in env {
                    cmd.env(k, v);
                }
                let transport = match rmcp::transport::child_process::TokioChildProcess::new(cmd) {
                    Ok(t) => t,
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to spawn child process '{}': {}", command, e));
                    }
                };

                let service = rmcp::ServiceExt::serve(handler, transport)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to serve stdio MCP client '{}': {}", name, e))?;

                // Fetch initial tools
                let tools = service.peer().list_all_tools().await
                    .map_err(|e| anyhow::anyhow!("Failed to list tools for MCP server '{}': {}", name, e))?;

                let mcp_tools = tools
                    .into_iter()
                    .map(|t| rig::tool::rmcp::McpTool::from_mcp_server(t, service.peer().clone()))
                    .collect::<Vec<_>>();

                reg.tools.write().unwrap().insert(name.clone(), mcp_tools);

                // Keep service running in background
                let name_clone = name.clone();
                let service_handle = tokio::spawn(async move {
                    if let Err(e) = service.waiting().await {
                        log::error!("MCP service '{}' stopped with error: {}", name_clone, e);
                    }
                });
                reg.services.write().unwrap().push(service_handle);
            }
            McpToolConfig::Sse { url } => {
                let transport = rmcp::transport::StreamableHttpClientTransport::from_uri(url.as_str());

                let service = rmcp::ServiceExt::serve(handler, transport)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to serve SSE MCP client '{}' at '{}': {}", name, url, e))?;

                // Fetch initial tools
                let tools = service.peer().list_all_tools().await
                    .map_err(|e| anyhow::anyhow!("Failed to list tools for SSE MCP server '{}': {}", name, e))?;

                let mcp_tools = tools
                    .into_iter()
                    .map(|t| rig::tool::rmcp::McpTool::from_mcp_server(t, service.peer().clone()))
                    .collect::<Vec<_>>();

                reg.tools.write().unwrap().insert(name.clone(), mcp_tools);

                // Keep service running in background
                let name_clone = name.clone();
                let service_handle = tokio::spawn(async move {
                    if let Err(e) = service.waiting().await {
                        log::error!("MCP service '{}' stopped with error: {}", name_clone, e);
                    }
                });
                reg.services.write().unwrap().push(service_handle);
            }
        }
    }

    Ok(())
}

pub fn get_mcp_tools() -> Vec<Box<dyn rig::tool::ToolDyn>> {
    let reg = registry();
    let guard = reg.tools.read().unwrap();
    let mut result = Vec::new();
    for tools in guard.values() {
        for tool in tools {
            result.push(Box::new(tool.clone()) as Box<dyn rig::tool::ToolDyn>);
        }
    }
    result
}
