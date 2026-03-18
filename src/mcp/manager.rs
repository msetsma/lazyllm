use std::collections::HashMap;
use std::sync::Arc;

use crate::config::types::McpServerConfig;
use crate::llm::types::ToolDefinition;
use crate::ui::components::tool_panel::ServerTools;

use super::client::McpClient;
use super::types::{McpError, ToolCallResult, ToolInfo};

/// Manages multiple MCP server connections.
pub struct McpManager {
    /// Active MCP clients, keyed by server name.
    clients: HashMap<String, Arc<McpClient>>,
    /// Tool name → server name routing.
    tool_routing: HashMap<String, String>,
    /// All available tools with their info.
    all_tools: Vec<(String, ToolInfo)>, // (server_name, tool_info)
}

impl McpManager {
    /// Spawn all configured MCP servers in parallel.
    /// Logs and skips any servers that fail to start.
    pub async fn new(configs: &[McpServerConfig]) -> Self {
        let mut clients = HashMap::new();
        let mut tool_routing = HashMap::new();
        let mut all_tools = Vec::new();

        // Spawn all servers in parallel
        let futures: Vec<_> = configs
            .iter()
            .map(|config| {
                let name = config.name.clone();
                let command = config.command.clone();
                let args = config.args.clone();
                let env = config.env.clone();
                async move {
                    tracing::info!("Spawning MCP server: {name}");
                    match McpClient::spawn(&name, &command, &args, &env).await {
                        Ok(client) => {
                            tracing::info!("MCP server '{name}' started successfully");
                            Some((name, client))
                        }
                        Err(e) => {
                            tracing::warn!("Failed to start MCP server '{name}': {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        for result in results {
            if let Some((name, client)) = result {
                let client = Arc::new(client);

                // Discover tools
                match client.list_tools().await {
                    Ok(tools) => {
                        tracing::info!(
                            "MCP server '{}' provides {} tools",
                            name,
                            tools.len()
                        );
                        for tool in &tools {
                            tool_routing.insert(tool.name.clone(), name.clone());
                            all_tools.push((name.clone(), tool.clone()));
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to list tools from MCP server '{}': {e}",
                            name
                        );
                    }
                }

                clients.insert(name, client);
            }
        }

        Self {
            clients,
            tool_routing,
            all_tools,
        }
    }

    /// Get tool definitions suitable for passing to LLM providers.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.all_tools
            .iter()
            .map(|(_, tool)| ToolDefinition {
                name: tool.name.clone(),
                description: tool.description.clone().unwrap_or_default(),
                input_schema: tool
                    .input_schema
                    .clone()
                    .unwrap_or(serde_json::json!({"type": "object"})),
            })
            .collect()
    }

    /// Get per-server tool groupings for the UI ToolPanel.
    pub fn server_tools(&self) -> Vec<ServerTools> {
        let mut server_map: HashMap<&str, Vec<String>> = HashMap::new();

        for (server_name, tool) in &self.all_tools {
            server_map
                .entry(server_name.as_str())
                .or_default()
                .push(tool.name.clone());
        }

        server_map
            .into_iter()
            .map(|(name, tools)| ServerTools {
                server_name: name.to_string(),
                tools,
            })
            .collect()
    }

    /// Call a tool by name, routing to the correct server.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<ToolCallResult, McpError> {
        let server_name = self
            .tool_routing
            .get(name)
            .ok_or_else(|| McpError::Transport(format!("No server found for tool '{name}'")))?;

        let client = self
            .clients
            .get(server_name)
            .ok_or_else(|| McpError::Transport(format!("Server '{server_name}' not connected")))?;

        client.call_tool(name, arguments).await
    }

    /// Number of connected servers.
    pub fn server_count(&self) -> usize {
        self.clients.len()
    }

    /// Whether any tools are available.
    pub fn has_tools(&self) -> bool {
        !self.all_tools.is_empty()
    }

    /// Shut down all MCP servers.
    pub async fn shutdown_all(&self) {
        for (name, client) in &self.clients {
            tracing::info!("Shutting down MCP server: {name}");
            client.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_config_creates_empty_manager() {
        let manager = McpManager::new(&[]).await;
        assert_eq!(manager.server_count(), 0);
        assert!(!manager.has_tools());
        assert!(manager.tool_definitions().is_empty());
        assert!(manager.server_tools().is_empty());
    }

    #[tokio::test]
    async fn call_unknown_tool_returns_error() {
        let manager = McpManager::new(&[]).await;
        let result = manager
            .call_tool("nonexistent", serde_json::json!({}))
            .await;
        assert!(result.is_err());
    }
}
