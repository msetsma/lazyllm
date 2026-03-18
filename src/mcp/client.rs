use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, oneshot};
use tokio::time::{Duration, timeout};

use super::types::*;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);

/// MCP client that communicates with a single server over stdio.
pub struct McpClient {
    server_name: String,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<tokio::process::ChildStdin>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    next_id: AtomicU64,
    tools: Mutex<Vec<ToolInfo>>,
}

impl McpClient {
    /// Spawn an MCP server process and initialize the connection.
    pub async fn spawn(
        name: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, McpError> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| McpError::Transport(format!("Failed to spawn {command}: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("No stdin on child process".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("No stdout on child process".to_string()))?;

        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Background reader task: routes responses by ID
        let pending_clone = Arc::clone(&pending);
        let server_name = name.to_string();
        let reader_name = server_name.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                let resp: JsonRpcResponse = match serde_json::from_str(&line) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::debug!("[{reader_name}] ignoring non-JSON line: {e}");
                        continue;
                    }
                };

                if let Some(id) = resp.id {
                    let mut map = pending_clone.lock().await;
                    if let Some(sender) = map.remove(&id) {
                        let _ = sender.send(resp);
                    }
                }
                // Notifications (no id) are ignored for now
            }

            tracing::debug!("[{reader_name}] stdout reader exited");
        });

        let client = Self {
            server_name,
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(Some(stdin)),
            pending,
            next_id: AtomicU64::new(1),
            tools: Mutex::new(Vec::new()),
        };

        client.initialize().await?;
        Ok(client)
    }

    /// Send the initialize handshake + initialized notification.
    async fn initialize(&self) -> Result<(), McpError> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "lazyllm",
                "version": "0.1.0"
            }
        });

        let resp = self
            .request_with_timeout("initialize", Some(params), INITIALIZE_TIMEOUT)
            .await?;

        tracing::info!(
            "[{}] MCP server initialized: {:?}",
            self.server_name,
            resp.result
        );

        // Send initialized notification
        self.notify("notifications/initialized", None).await?;

        Ok(())
    }

    /// List available tools from this server.
    pub async fn list_tools(&self) -> Result<Vec<ToolInfo>, McpError> {
        let resp = self
            .request_with_timeout("tools/list", None, REQUEST_TIMEOUT)
            .await?;

        let result = resp.result.unwrap_or(serde_json::Value::Null);
        let tools: Vec<ToolInfo> = result
            .get("tools")
            .and_then(|t| serde_json::from_value(t.clone()).ok())
            .unwrap_or_default();

        *self.tools.lock().await = tools.clone();
        Ok(tools)
    }

    /// Call a tool on this server.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<ToolCallResult, McpError> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });

        let resp = self
            .request_with_timeout("tools/call", Some(params), REQUEST_TIMEOUT)
            .await?;

        let result = resp.result.unwrap_or(serde_json::Value::Null);

        let is_error = result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
        let content = result
            .get("content")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            item.get("text").and_then(|t| t.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        Ok(ToolCallResult { content, is_error })
    }

    /// Shut down the server gracefully.
    pub async fn shutdown(&self) {
        // Best-effort shutdown notification
        let _ = self.notify("notifications/cancelled", None).await;

        let mut stdin = self.stdin.lock().await;
        *stdin = None; // Close stdin to signal EOF

        let mut child = self.child.lock().await;
        if let Some(mut c) = child.take() {
            // Give the process a moment to exit, then kill
            match timeout(Duration::from_secs(2), c.wait()).await {
                Ok(_) => tracing::debug!("[{}] MCP server exited", self.server_name),
                Err(_) => {
                    let _ = c.kill().await;
                    tracing::debug!("[{}] MCP server killed", self.server_name);
                }
            }
        }
    }

    /// Send a JSON-RPC request and wait for the response with a timeout.
    async fn request_with_timeout(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
        dur: Duration,
    ) -> Result<JsonRpcResponse, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);

        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().await;
            map.insert(id, tx);
        }

        self.send(&request).await?;

        let resp = timeout(dur, rx)
            .await
            .map_err(|_| McpError::Timeout)?
            .map_err(|_| McpError::Transport("Response channel closed".to_string()))?;

        if let Some(err) = resp.error {
            return Err(McpError::Protocol(err));
        }

        Ok(resp)
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), McpError> {
        let notification = JsonRpcNotification::new(method, params);
        let mut buf = serde_json::to_vec(&notification)
            .map_err(|e| McpError::Transport(format!("Failed to serialize: {e}")))?;
        buf.push(b'\n');

        let mut stdin = self.stdin.lock().await;
        let stdin = stdin
            .as_mut()
            .ok_or(McpError::Shutdown)?;

        stdin
            .write_all(&buf)
            .await
            .map_err(|e| McpError::Transport(format!("Failed to write: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::Transport(format!("Failed to flush: {e}")))?;

        Ok(())
    }

    /// Send a serializable message to the server's stdin.
    async fn send(&self, msg: &JsonRpcRequest) -> Result<(), McpError> {
        let mut buf = serde_json::to_vec(msg)
            .map_err(|e| McpError::Transport(format!("Failed to serialize: {e}")))?;
        buf.push(b'\n');

        let mut stdin = self.stdin.lock().await;
        let stdin = stdin
            .as_mut()
            .ok_or(McpError::Shutdown)?;

        stdin
            .write_all(&buf)
            .await
            .map_err(|e| McpError::Transport(format!("Failed to write: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::Transport(format!("Failed to flush: {e}")))?;

        Ok(())
    }

    pub fn server_name(&self) -> &str {
        &self.server_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_nonexistent_command_returns_error() {
        let result = McpClient::spawn(
            "test",
            "/nonexistent/binary",
            &[],
            &HashMap::new(),
        )
        .await;

        assert!(result.is_err());
        match result {
            Err(McpError::Transport(_)) => {} // expected
            Err(other) => panic!("Expected Transport error, got: {other}"),
            Ok(_) => panic!("Expected error"),
        }
    }
}
