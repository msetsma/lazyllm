use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::LlmProvider;
use super::streaming::{check_http_error, stream_sse_response};
use super::types::{ChatRequest, LlmError, ModelInfo, StreamChunk, TokenUsage};

#[allow(dead_code)]
const GOOGLE_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Google Gemini API provider with SSE streaming.
#[derive(Debug)]
pub struct GoogleProvider {
    name: String,
    api_key: String,
    base_url: String,
    models: Vec<ModelInfo>,
    client: Client,
}

impl GoogleProvider {
    pub fn new(
        name: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        models: Vec<ModelInfo>,
    ) -> Self {
        Self {
            name: name.into(),
            api_key: api_key.into(),
            base_url: base_url.into(),
            models,
            client: Client::new(),
        }
    }

    fn stream_url(&self, model: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!(
            "{base}/models/{model}:streamGenerateContent?alt=sse&key={}",
            self.api_key
        )
    }
}

/// Google Gemini request body.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Serialize, Clone)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Clone)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

/// Gemini streaming response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiStreamResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    usage_metadata: Option<GeminiUsageMetadata>,
    #[serde(default)]
    error: Option<GeminiError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    #[serde(default)]
    prompt_token_count: Option<u32>,
    #[serde(default)]
    candidates_token_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    content: Option<GeminiResponseContent>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiResponsePart>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponsePart {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiError {
    message: String,
}

/// Gemini error response for non-streaming errors.
#[derive(Debug, Deserialize)]
struct GeminiErrorResponse {
    error: GeminiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorDetail {
    message: String,
}

impl From<&ChatRequest> for GeminiRequest {
    fn from(req: &ChatRequest) -> Self {
        let mut system_instruction = None;
        let mut contents = Vec::new();

        for msg in &req.messages {
            let role = msg.role.as_str();

            if role == "system" {
                system_instruction = Some(GeminiContent {
                    role: "user".to_string(), // Gemini uses "user" role for system instructions
                    parts: vec![GeminiPart {
                        text: msg.content.clone(),
                    }],
                });
            } else {
                // Gemini uses "user" and "model" instead of "user" and "assistant"
                let gemini_role = if role == "assistant" {
                    "model".to_string()
                } else {
                    role.to_string()
                };

                contents.push(GeminiContent {
                    role: gemini_role,
                    parts: vec![GeminiPart {
                        text: msg.content.clone(),
                    }],
                });
            }
        }

        let generation_config =
            if req.temperature.is_some() || req.max_tokens.is_some() {
                Some(GeminiGenerationConfig {
                    temperature: req.temperature,
                    max_output_tokens: req.max_tokens,
                })
            } else {
                None
            };

        Self {
            contents,
            system_instruction,
            generation_config,
        }
    }
}

/// Parse a single SSE data line from the Gemini stream.
fn parse_gemini_sse(line: &str) -> Option<StreamChunk> {
    let data = line.strip_prefix("data: ")?;

    let response: GeminiStreamResponse = match serde_json::from_str(data) {
        Ok(r) => r,
        Err(e) => return Some(StreamChunk::Error(format!("Failed to parse chunk: {e}"))),
    };

    if let Some(error) = response.error {
        return Some(StreamChunk::Error(error.message));
    }

    if let Some(candidate) = response.candidates.first() {
        if let Some(content) = &candidate.content {
            for part in &content.parts {
                if let Some(text) = &part.text
                    && !text.is_empty()
                {
                    return Some(StreamChunk::Delta(text.clone()));
                }
            }
        }

        if candidate.finish_reason.as_deref() == Some("STOP") {
            // If usage metadata is present, emit it; stream-end will send Done
            if let Some(usage) = &response.usage_metadata {
                let input = usage.prompt_token_count.unwrap_or(0);
                let output = usage.candidates_token_count.unwrap_or(0);
                return Some(StreamChunk::Usage(TokenUsage::new(input, output)));
            }
            return Some(StreamChunk::Done);
        }
    }

    None
}

#[async_trait]
impl LlmProvider for GoogleProvider {
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
        let body = GeminiRequest::from(&request);
        let url = self.stream_url(&request.model);

        tracing::debug!("Sending Gemini chat request");

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        let response = check_http_error(response, |body| {
            serde_json::from_str::<GeminiErrorResponse>(body)
                .map(|e| e.error.message)
                .ok()
        })
        .await?;

        stream_sse_response(response, &tx, parse_gemini_sse, |_| false).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Message;

    #[test]
    fn google_provider_name() {
        let provider = GoogleProvider::new(
            "google",
            "test-key",
            GOOGLE_API_URL,
            vec![ModelInfo::new("gemini-2.0-flash")],
        );
        assert_eq!(provider.name(), "google");
    }

    #[test]
    fn google_provider_models() {
        let provider = GoogleProvider::new(
            "google",
            "test-key",
            GOOGLE_API_URL,
            vec![
                ModelInfo::new("gemini-2.0-flash"),
                ModelInfo::new("gemini-2.5-pro"),
            ],
        );
        let models = provider.available_models();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gemini-2.0-flash");
    }

    #[test]
    fn stream_url_construction() {
        let provider = GoogleProvider::new(
            "google",
            "my-api-key",
            GOOGLE_API_URL,
            vec![],
        );
        let url = provider.stream_url("gemini-2.0-flash");
        assert!(url.contains("models/gemini-2.0-flash:streamGenerateContent"));
        assert!(url.contains("key=my-api-key"));
        assert!(url.contains("alt=sse"));
    }

    #[test]
    fn gemini_request_extracts_system_instruction() {
        let chat_req = ChatRequest::new(
            "gemini-2.0-flash",
            vec![
                Message::system("be helpful"),
                Message::user("hello"),
            ],
        );

        let req = GeminiRequest::from(&chat_req);
        assert!(req.system_instruction.is_some());
        assert_eq!(req.system_instruction.unwrap().parts[0].text, "be helpful");
        assert_eq!(req.contents.len(), 1);
        assert_eq!(req.contents[0].role, "user");
    }

    #[test]
    fn gemini_request_maps_assistant_to_model() {
        let chat_req = ChatRequest::new(
            "gemini-2.0-flash",
            vec![
                Message::user("hello"),
                Message::assistant("hi there"),
                Message::user("how are you"),
            ],
        );

        let req = GeminiRequest::from(&chat_req);
        assert_eq!(req.contents.len(), 3);
        assert_eq!(req.contents[0].role, "user");
        assert_eq!(req.contents[1].role, "model");
        assert_eq!(req.contents[2].role, "user");
    }

    #[test]
    fn gemini_request_no_system() {
        let chat_req = ChatRequest::new(
            "gemini-2.0-flash",
            vec![Message::user("hello")],
        );

        let req = GeminiRequest::from(&chat_req);
        assert!(req.system_instruction.is_none());
    }

    #[test]
    fn gemini_request_with_generation_config() {
        let chat_req = ChatRequest::new("gemini-2.0-flash", vec![Message::user("hi")])
            .with_temperature(0.7)
            .with_max_tokens(500);

        let req = GeminiRequest::from(&chat_req);
        let config = req.generation_config.unwrap();
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.max_output_tokens, Some(500));
    }

    #[test]
    fn gemini_request_serializes_correctly() {
        let chat_req = ChatRequest::new("gemini-2.0-flash", vec![Message::user("hi")]);
        let req = GeminiRequest::from(&chat_req);
        let json = serde_json::to_value(&req).unwrap();

        assert!(json["contents"].is_array());
        assert_eq!(json["contents"][0]["role"], "user");
        assert_eq!(json["contents"][0]["parts"][0]["text"], "hi");
        assert!(json.get("systemInstruction").is_none());
        assert!(json.get("generationConfig").is_none());
    }

    #[test]
    fn parse_gemini_delta() {
        let line = r#"data: {"candidates":[{"content":{"parts":[{"text":"Hello"}],"role":"model"},"index":0}]}"#;
        let chunk = parse_gemini_sse(line).unwrap();
        assert_eq!(chunk, StreamChunk::Delta("Hello".to_string()));
    }

    #[test]
    fn parse_gemini_stop() {
        let line = r#"data: {"candidates":[{"content":{"parts":[{"text":""}],"role":"model"},"finishReason":"STOP","index":0}]}"#;
        let chunk = parse_gemini_sse(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }

    #[test]
    fn parse_gemini_error() {
        let line = r#"data: {"error":{"message":"quota exceeded"}}"#;
        let chunk = parse_gemini_sse(line).unwrap();
        assert_eq!(chunk, StreamChunk::Error("quota exceeded".to_string()));
    }

    #[test]
    fn parse_gemini_non_data_line_returns_none() {
        assert!(parse_gemini_sse("event: message").is_none());
        assert!(parse_gemini_sse("").is_none());
    }

    #[test]
    fn parse_gemini_invalid_json_returns_error() {
        let line = "data: {invalid}";
        let chunk = parse_gemini_sse(line).unwrap();
        match chunk {
            StreamChunk::Error(msg) => assert!(msg.contains("Failed to parse")),
            other => panic!("Expected Error, got: {:?}", other),
        }
    }

    #[test]
    fn parse_gemini_stop_with_usage_metadata() {
        let line = r#"data: {"candidates":[{"content":{"parts":[{"text":""}],"role":"model"},"finishReason":"STOP","index":0}],"usageMetadata":{"promptTokenCount":15,"candidatesTokenCount":200,"totalTokenCount":215}}"#;
        let chunk = parse_gemini_sse(line).unwrap();
        assert_eq!(
            chunk,
            StreamChunk::Usage(super::super::types::TokenUsage::new(15, 200))
        );
    }
}
