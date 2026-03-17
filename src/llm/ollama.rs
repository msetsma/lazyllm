use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::LlmProvider;
use super::types::{ChatRequest, LlmError, ModelInfo, StreamChunk};

const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";

/// Ollama provider using the native Ollama chat API with streaming.
///
/// While Ollama supports an OpenAI-compatible endpoint, this implementation
/// uses the native `/api/chat` endpoint for better compatibility with
/// Ollama-specific features.
#[derive(Debug)]
pub struct OllamaProvider {
    name: String,
    base_url: String,
    models: Vec<ModelInfo>,
    client: Client,
}

impl OllamaProvider {
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
        models: Vec<ModelInfo>,
    ) -> Self {
        Self {
            name: name.into(),
            base_url: base_url.into(),
            models,
            client: Client::new(),
        }
    }

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
                role: serde_json::to_value(&m.role)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
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
        return Some(StreamChunk::Done);
    }

    if let Some(msg) = chunk.message {
        if !msg.content.is_empty() {
            return Some(StreamChunk::Delta(msg.content));
        }
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

        let status = response.status();
        if !status.is_success() {
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(LlmError::ApiError {
                status: status.as_u16(),
                message: body_text,
            });
        }

        // Ollama streams NDJSON (one JSON object per line)
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            let bytes = chunk_result.map_err(|e| LlmError::NetworkError(e.to_string()))?;
            let text = String::from_utf8_lossy(&bytes);
            buffer.push_str(&text);

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if let Some(chunk) = parse_ollama_line(&line) {
                    let is_done = chunk == StreamChunk::Done;
                    if tx.send(chunk).is_err() {
                        return Ok(());
                    }
                    if is_done {
                        return Ok(());
                    }
                }
            }
        }

        tx.send(StreamChunk::Done).ok();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Message;

    #[test]
    fn ollama_provider_name() {
        let provider = OllamaProvider::new(
            "ollama",
            DEFAULT_OLLAMA_URL,
            vec![ModelInfo::new("llama3.2")],
        );
        assert_eq!(provider.name(), "ollama");
    }

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

    #[test]
    fn chat_url_construction() {
        let provider = OllamaProvider::new("ollama", "http://localhost:11434", vec![]);
        assert_eq!(provider.chat_url(), "http://localhost:11434/api/chat");

        let provider = OllamaProvider::new("ollama", "http://localhost:11434/", vec![]);
        assert_eq!(provider.chat_url(), "http://localhost:11434/api/chat");
    }

    #[test]
    fn local_constructor_uses_default_url() {
        let provider = OllamaProvider::local("ollama", vec!["llama3.2".to_string()]);
        assert_eq!(provider.chat_url(), "http://localhost:11434/api/chat");
    }

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

    #[test]
    fn ollama_request_serializes_correctly() {
        let chat_req = ChatRequest::new("llama3.2", vec![Message::user("hi")]);
        let req = OllamaRequest::from(&chat_req);
        let json = serde_json::to_value(&req).unwrap();

        assert_eq!(json["model"], "llama3.2");
        assert_eq!(json["stream"], true);
        assert!(json.get("options").is_none());
    }

    #[test]
    fn parse_ollama_delta() {
        let line = r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":"Hello"},"done":false}"#;
        let chunk = parse_ollama_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Delta("Hello".to_string()));
    }

    #[test]
    fn parse_ollama_done() {
        let line = r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":""},"done":true}"#;
        let chunk = parse_ollama_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }

    #[test]
    fn parse_ollama_error() {
        let line = r#"{"error":"model not found"}"#;
        let chunk = parse_ollama_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Error("model not found".to_string()));
    }

    #[test]
    fn parse_ollama_empty_line_returns_none() {
        assert!(parse_ollama_line("").is_none());
        assert!(parse_ollama_line("  ").is_none());
    }

    #[test]
    fn parse_ollama_invalid_json_returns_error() {
        let chunk = parse_ollama_line("{bad json}").unwrap();
        match chunk {
            StreamChunk::Error(msg) => assert!(msg.contains("Failed to parse")),
            other => panic!("Expected Error, got: {:?}", other),
        }
    }
}
