use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 request.
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 notification (no id).
#[derive(Debug, Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<u64>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

/// MCP tool information returned by tools/list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Option<Value>,
}

/// Parameters for calling a tool via tools/call.
#[derive(Debug, Serialize)]
pub struct ToolCallParams {
    pub name: String,
    pub arguments: Value,
}

/// Result from a tools/call invocation.
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub content: String,
    pub is_error: bool,
}

/// Content item in a tools/call response.
#[derive(Debug, Deserialize)]
pub struct ToolResultContent {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub text: Option<String>,
}

/// MCP client error.
#[derive(Debug)]
pub enum McpError {
    /// Failed to spawn or communicate with the server process.
    Transport(String),
    /// The server returned a JSON-RPC error.
    Protocol(JsonRpcError),
    /// Request timed out.
    Timeout,
    /// The server has been shut down.
    Shutdown,
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::Transport(msg) => write!(f, "MCP transport error: {msg}"),
            McpError::Protocol(err) => write!(f, "MCP protocol error: {err}"),
            McpError::Timeout => write!(f, "MCP request timed out"),
            McpError::Shutdown => write!(f, "MCP client has been shut down"),
        }
    }
}

impl std::error::Error for McpError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_rpc_request_serializes() {
        let req = JsonRpcRequest::new(1, "tools/list", None);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 1);
        assert_eq!(json["method"], "tools/list");
        assert!(json.get("params").is_none());
    }

    #[test]
    fn json_rpc_request_with_params() {
        let params = serde_json::json!({"name": "test"});
        let req = JsonRpcRequest::new(2, "tools/call", Some(params));
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["params"]["name"], "test");
    }

    #[test]
    fn json_rpc_notification_serializes() {
        let notif = JsonRpcNotification::new("notifications/initialized", None);
        let json = serde_json::to_value(&notif).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["method"], "notifications/initialized");
        assert!(json.get("id").is_none());
    }

    #[test]
    fn json_rpc_response_deserializes() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, Some(1));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn json_rpc_error_response_deserializes() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid request"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32600);
        assert_eq!(err.message, "Invalid request");
    }

    #[test]
    fn tool_info_deserializes() {
        let json = r#"{"name":"read_file","description":"Read a file","inputSchema":{"type":"object"}}"#;
        let info: ToolInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "read_file");
        assert_eq!(info.description.as_deref(), Some("Read a file"));
        assert!(info.input_schema.is_some());
    }

    #[test]
    fn tool_call_params_serializes() {
        let params = ToolCallParams {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
        };
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["name"], "read_file");
        assert_eq!(json["arguments"]["path"], "/tmp/test.txt");
    }

    #[test]
    fn mcp_error_display() {
        let err = McpError::Transport("connection refused".to_string());
        assert!(err.to_string().contains("connection refused"));

        let err = McpError::Timeout;
        assert!(err.to_string().contains("timed out"));

        let err = McpError::Shutdown;
        assert!(err.to_string().contains("shut down"));
    }
}
