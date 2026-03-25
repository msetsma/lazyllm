use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::LlmProvider;
use super::streaming::{check_http_error, stream_sse_response};
use super::types::{ChatRequest, LlmError, ModelInfo, StreamChunk, TokenUsage};

const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";

define_provider!(OllamaProvider, no_key);

impl OllamaProvider {
    /// Create with default localhost URL.
    pub fn local(name: impl Into<String>, models: Vec<String>) -> Self {
        let model_infos = models.into_iter().map(ModelInfo::new).collect();
        Self::new(name, DEFAULT_OLLAMA_URL, model_infos)
    }

    fn chat_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/api/chat")
    }
}

/// Ollama native chat request.
#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Debug, Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

/// Ollama streaming response chunk (one JSON object per line).
#[derive(Debug, Deserialize)]
struct OllamaStreamChunk {
    #[serde(default)]
    message: Option<OllamaResponseMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    #[serde(default)]
    content: String,
}

impl From<&ChatRequest> for OllamaRequest {
    fn from(req: &ChatRequest) -> Self {
        let messages = req
            .messages
            .iter()
            .map(|m| OllamaMessage {
                role: m.role.as_str().to_string(),
                content: m.content.clone(),
            })
            .collect();

        let options = if req.temperature.is_some() || req.max_tokens.is_some() {
            Some(OllamaOptions {
                temperature: req.temperature,
                num_predict: req.max_tokens,
            })
        } else {
            None
        };

        Self {
            model: req.model.clone(),
            messages,
            stream: true,
            options,
        }
    }
}

/// Parse a single line from the Ollama NDJSON stream.
fn parse_ollama_line(line: &str) -> Option<StreamChunk> {
    if line.trim().is_empty() {
        return None;
    }

    let chunk: OllamaStreamChunk = match serde_json::from_str(line) {
        Ok(c) => c,
        Err(e) => return Some(StreamChunk::Error(format!("Failed to parse chunk: {e}"))),
    };

    if let Some(error) = chunk.error {
        return Some(StreamChunk::Error(error));
    }

    if chunk.done {
        // Ollama includes usage stats in the final done chunk
        if chunk.prompt_eval_count.is_some() || chunk.eval_count.is_some() {
            let input = chunk.prompt_eval_count.unwrap_or(0);
            let output = chunk.eval_count.unwrap_or(0);
            // We can't return two chunks from one parse call, so we embed usage
            // in Done by sending Usage first — the caller will get Done on the next line.
            return Some(StreamChunk::Usage(TokenUsage::new(input, output)));
        }
        return Some(StreamChunk::Done);
    }

    if let Some(msg) = chunk.message
        && !msg.content.is_empty()
    {
        return Some(StreamChunk::Delta(msg.content));
    }

    None
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        self.models.clone()
    }

    async fn chat(
        &self,
        request: ChatRequest,
        tx: mpsc::UnboundedSender<StreamChunk>,
    ) -> Result<(), LlmError> {
        let body = OllamaRequest::from(&request);
        let url = self.chat_url();

        tracing::debug!("Sending Ollama chat request to {url}");

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        let response = check_http_error(response, |body| Some(body.to_string())).await?;

        // Ollama streams NDJSON (one JSON object per line)
        stream_sse_response(response, &tx, parse_ollama_line, |_| false).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Message;

    /// Verifies the provider reports the correct name for identification.
    #[test]
    fn ollama_provider_name() {
        let provider = OllamaProvider::new(
            "ollama",
            DEFAULT_OLLAMA_URL,
            vec![ModelInfo::new("llama3.2")],
        );
        assert_eq!(provider.name(), "ollama");
    }

    /// Ensures model IDs from the local() convenience constructor are preserved.
    #[test]
    fn ollama_provider_models() {
        let provider = OllamaProvider::local(
            "ollama",
            vec!["llama3.2".to_string(), "mistral".to_string()],
        );
        let models = provider.available_models();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "llama3.2");
        assert_eq!(models[1].id, "mistral");
    }

    /// Ensures the chat URL is normalized with or without trailing slash.
    #[test]
    fn chat_url_construction() {
        let provider = OllamaProvider::new("ollama", "http://localhost:11434", vec![]);
        assert_eq!(provider.chat_url(), "http://localhost:11434/api/chat");

        let provider = OllamaProvider::new("ollama", "http://localhost:11434/", vec![]);
        assert_eq!(provider.chat_url(), "http://localhost:11434/api/chat");
    }

    /// Verifies the local() constructor uses the default Ollama URL.
    #[test]
    fn local_constructor_uses_default_url() {
        let provider = OllamaProvider::local("ollama", vec!["llama3.2".to_string()]);
        assert_eq!(provider.chat_url(), "http://localhost:11434/api/chat");
    }

    /// Verifies ChatRequest messages are mapped to Ollama's format with roles preserved.
    #[test]
    fn ollama_request_from_chat_request() {
        let chat_req = ChatRequest::new(
            "llama3.2",
            vec![
                Message::system("be concise"),
                Message::user("hello"),
            ],
        );

        let req = OllamaRequest::from(&chat_req);
        assert_eq!(req.model, "llama3.2");
        assert!(req.stream);
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert_eq!(req.messages[1].role, "user");
        assert!(req.options.is_none());
    }

    /// Verifies temperature and max_tokens are mapped to Ollama's options format.
    #[test]
    fn ollama_request_with_options() {
        let chat_req = ChatRequest::new("llama3.2", vec![Message::user("hi")])
            .with_temperature(0.5)
            .with_max_tokens(200);

        let req = OllamaRequest::from(&chat_req);
        let opts = req.options.unwrap();
        assert_eq!(opts.temperature, Some(0.5));
        assert_eq!(opts.num_predict, Some(200));
    }

    /// Verifies JSON serialization omits options when not set.
    #[test]
    fn ollama_request_serializes_correctly() {
        let chat_req = ChatRequest::new("llama3.2", vec![Message::user("hi")]);
        let req = OllamaRequest::from(&chat_req);
        let json = serde_json::to_value(&req).unwrap();

        assert_eq!(json["model"], "llama3.2");
        assert_eq!(json["stream"], true);
        assert!(json.get("options").is_none());
    }

    /// Verifies NDJSON lines with content are parsed into Delta chunks.
    #[test]
    fn parse_ollama_delta() {
        let line = r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":"Hello"},"done":false}"#;
        let chunk = parse_ollama_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Delta("Hello".to_string()));
    }

    /// Verifies done:true signals stream completion.
    #[test]
    fn parse_ollama_done() {
        let line = r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":""},"done":true}"#;
        let chunk = parse_ollama_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }

    /// Verifies Ollama error responses are parsed into Error chunks.
    #[test]
    fn parse_ollama_error() {
        let line = r#"{"error":"model not found"}"#;
        let chunk = parse_ollama_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Error("model not found".to_string()));
    }

    /// Ensures empty/whitespace NDJSON lines are silently skipped.
    #[test]
    fn parse_ollama_empty_line_returns_none() {
        assert!(parse_ollama_line("").is_none());
        assert!(parse_ollama_line("  ").is_none());
    }

    /// Ensures malformed JSON produces an Error chunk rather than panicking.
    #[test]
    fn parse_ollama_invalid_json_returns_error() {
        let chunk = parse_ollama_line("{bad json}").unwrap();
        match chunk {
            StreamChunk::Error(msg) => assert!(msg.contains("Failed to parse")),
            other => panic!("Expected Error, got: {:?}", other),
        }
    }

    /// Verifies token usage (prompt_eval_count, eval_count) is extracted from done responses.
    #[test]
    fn parse_ollama_done_with_usage() {
        let line = r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":""},"done":true,"prompt_eval_count":28,"eval_count":150,"total_duration":1234}"#;
        let chunk = parse_ollama_line(line).unwrap();
        assert_eq!(
            chunk,
            StreamChunk::Usage(super::super::types::TokenUsage::new(28, 150))
        );
    }
}
