use crate::config::Config;
use crate::tools::tool_filter::ToolNameFilter;
use derive_more::{Deref, Display, From};
use itertools::Itertools;
use rig::completion::ToolDefinition;
use rig::tool::rmcp::McpTool;
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use rmcp::service::NotificationContext;
use rmcp::{RoleClient, ServiceExt};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Serialize, Deserialize, Deref)]
pub struct McpToolSetConfigs(HashMap<McpToolSetName, McpToolSetConfig>);

#[derive(Debug, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Display, Serialize, Deserialize)]
pub struct McpToolSetName(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpToolSetConfig {
    #[serde(rename = "stdio")]
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        tool_filter: Option<ToolNameFilter>,
    },
    #[serde(rename = "sse")]
    Sse {
        url: String,
        tool_filter: Option<ToolNameFilter>,
    },
}

impl McpToolSetConfig {
    fn tool_filter(&self) -> &Option<ToolNameFilter> {
        match self {
            McpToolSetConfig::Stdio { tool_filter, .. } => tool_filter,
            McpToolSetConfig::Sse { tool_filter, .. } => tool_filter,
        }
    }
}

#[derive(Deref)]
pub struct McpRegistry {
    #[allow(unused)]
    config: &'static Config,
    #[deref]
    mcp_tool_set: Arc<RwLock<HashMap<McpToolSetName, McpToolSet>>>,
}

#[derive(Deref)]
pub struct McpToolSet {
    #[deref]
    inner: McpToolSetInnerShared,
    #[allow(unused)]
    join_handle: JoinHandle<()>,
}

impl McpToolSet {
    #[allow(unused)]
    async fn join(self) -> crate::Result<()> {
        let _ = self.join_handle.await?;
        Ok(())
    }
}

#[derive(Deref)]
pub struct McpToolSetInner {
    name: McpToolSetName,
    #[allow(unused)]
    config: McpToolSetConfig,
    #[deref]
    tools: RwLock<Vec<ProxiedMcpTool>>,
}

#[derive(Deref, Clone)]
pub struct McpToolSetInnerShared(Arc<McpToolSetInner>);

impl ToolNameFilter {
    fn mcp_tool_filter(&self, tool: rmcp::model::Tool) -> Option<rmcp::model::Tool> {
        let dst_tool_name = &tool.name;
        match self {
            ToolNameFilter::Accepts(tool_names) => {
                if tool_names
                    .iter()
                    .any(|it| it.eq_ignore_ascii_case(dst_tool_name))
                {
                    Some(tool)
                } else {
                    None
                }
            }
            ToolNameFilter::Rejects(tool_names) => {
                if tool_names
                    .iter()
                    .any(|it| it.eq_ignore_ascii_case(dst_tool_name))
                {
                    None
                } else {
                    Some(tool)
                }
            }
        }
    }
}

impl rmcp::handler::client::ClientHandler for McpToolSetInnerShared {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        rmcp::model::ClientInfo::new(
            rmcp::model::ClientCapabilities::default(),
            rmcp::model::Implementation::new("fastclaw", env!("CARGO_PKG_VERSION")),
        )
    }

    async fn on_tool_list_changed(&self, context: NotificationContext<RoleClient>) {
        log::info!(
            "MCP server '{}' notified tool list changed, re-fetching...",
            self.name
        );
        match context.peer.list_all_tools().await {
            Ok(tools) => {
                let mut guard = self.write().await;
                let tool_filter = self.config.tool_filter().clone().unwrap_or_default();
                *guard = tools
                    .into_iter()
                    .flat_map(|it| tool_filter.mcp_tool_filter(it))
                    .map(|it| McpTool::from_mcp_server(it, context.peer.clone()).into())
                    .collect::<Vec<_>>();
                log::info!(
                    "MCP server '{}' updated with {} tools.",
                    self.name,
                    guard.len()
                );
            }
            Err(e) => {
                log::warn!(
                    "Failed to re-fetch tools from MCP server '{}': {}",
                    self.name,
                    e
                );
            }
        }
    }
}

impl McpToolSet {
    async fn new(name: &McpToolSetName, config: &McpToolSetConfig) -> crate::Result<Self> {
        let (inner, service) = match config {
            McpToolSetConfig::Stdio {
                command,
                args,
                env,
                tool_filter: _,
            } => {
                let cmd = {
                    let mut cmd = rmcp::transport::child_process::which_command(command)?;
                    cmd.args(args).envs(env);
                    cmd
                };
                let transport = rmcp::transport::child_process::TokioChildProcess::new(cmd)?;
                let mcp_tool_set = McpToolSetInnerShared(Arc::new(McpToolSetInner {
                    name: name.clone(),
                    config: config.clone(),
                    tools: Default::default(),
                }));

                let (service, mcp_tools) = {
                    let mcp_tool_set = mcp_tool_set.clone();
                    let service = mcp_tool_set.serve(transport).await.map_err(|e| {
                        anyhow::anyhow!("Failed to serve for stdio MCP server '{}': {}", name, e)
                    })?;
                    let tools = service.peer().list_all_tools().await.map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to list tools for stdio MCP server '{}': {}",
                            name,
                            e
                        )
                    })?;
                    let tool_filter = config.tool_filter().clone().unwrap_or_default();
                    let mcp_tools = tools
                        .into_iter()
                        .flat_map(|it| tool_filter.mcp_tool_filter(it))
                        .map(|t| McpTool::from_mcp_server(t, service.peer().clone()).into())
                        .collect_vec();
                    (service, mcp_tools)
                };

                {
                    let mut guard = mcp_tool_set.write().await;
                    *guard = mcp_tools;
                }
                (mcp_tool_set, service)
            }
            McpToolSetConfig::Sse {
                url,
                tool_filter: _,
            } => {
                let transport =
                    rmcp::transport::StreamableHttpClientTransport::from_uri(url.as_str());
                let mcp_tool_set = McpToolSetInnerShared(Arc::new(McpToolSetInner {
                    name: name.clone(),
                    config: config.clone(),
                    tools: Default::default(),
                }));
                let (service, mcp_tools) = {
                    let mcp_tool_set = mcp_tool_set.clone();
                    let service = mcp_tool_set.serve(transport).await.map_err(|e| {
                        anyhow::anyhow!("Failed to serve for SSE MCP server '{}': {}", name, e)
                    })?;
                    let tools = service.peer().list_all_tools().await.map_err(|e| {
                        anyhow::anyhow!("Failed to list tools for SSE MCP server '{}': {}", name, e)
                    })?;
                    let tool_filter = config.tool_filter().clone().unwrap_or_default();
                    let mcp_tools = tools
                        .into_iter()
                        .flat_map(|it| tool_filter.mcp_tool_filter(it))
                        .map(|t| McpTool::from_mcp_server(t, service.peer().clone()).into())
                        .collect_vec();
                    (service, mcp_tools)
                };
                {
                    let mut guard = mcp_tool_set.write().await;
                    *guard = mcp_tools;
                }
                (mcp_tool_set, service)
            }
        };
        let join_handle = {
            // Keep service running in background
            let name_clone = name.clone();
            let join_handle = tokio::spawn(async move {
                if let Err(e) = service.waiting().await {
                    log::error!("MCP service '{}' stopped with error: {}", name_clone, e);
                }
            });
            join_handle
        };
        Ok(Self { inner, join_handle })
    }
}

impl McpRegistry {
    pub fn new(config: &'static Config) -> crate::Result<McpRegistry> {
        Ok(McpRegistry {
            config,
            mcp_tool_set: Default::default(),
        })
    }

    pub async fn init(self) -> crate::Result<McpRegistry> {
        for (name, config) in self.config.mcp_tools.iter().flat_map(|it| it.deref()) {
            let mcp_tool_set = Arc::clone(&self.mcp_tool_set);
            let _ = tokio::spawn(async move {
                loop {
                    match McpToolSet::new(name, config).await {
                        Ok(dst) => {
                            let mut guard = mcp_tool_set.write().await;
                            guard.insert(name.clone(), dst);
                            log::info!("Success to init mcp server '{name}'");
                            return;
                        }
                        Err(err) => {
                            log::warn!(
                                "Failed to init mcp server '{name}', err: {err}, retry in future"
                            );
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    }
                }
            });
        }
        Ok(self)
    }

    pub async fn tools(&self) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        let mut vec = vec![];
        let mcp_tool_set = self.mcp_tool_set.read().await;
        for (_, toolset) in mcp_tool_set.iter() {
            let guard = toolset.read().await;
            let mut tools = guard
                .iter()
                .map(|it| Box::new(it.clone()) as Box<dyn ToolDyn>)
                .collect_vec();
            vec.append(&mut tools);
        }
        Ok(vec)
    }
}

#[derive(Clone, From, Deref)]
pub struct ProxiedMcpTool(McpTool);

impl ToolDyn for ProxiedMcpTool {
    fn name(&self) -> String {
        self.deref().name()
    }

    fn definition<'a>(&'a self, prompt: String) -> WasmBoxedFuture<'a, ToolDefinition> {
        self.deref().definition(prompt)
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        self.deref().call(args)
    }
}
