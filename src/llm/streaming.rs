use futures::StreamExt;
use tokio::sync::mpsc;

use super::types::{LlmError, StreamChunk, TokenUsage};

/// Stream an SSE response, parsing each line and sending chunks to the channel.
///
/// - `parse_line`: converts a trimmed, non-empty line into an optional `StreamChunk`
/// - `skip_line`: returns true for lines that should be skipped (e.g., `event:` lines)
pub async fn stream_sse_response(
    response: reqwest::Response,
    tx: &mpsc::UnboundedSender<StreamChunk>,
    parse_line: impl Fn(&str) -> Option<StreamChunk>,
    skip_line: impl Fn(&str) -> bool,
) -> Result<(), LlmError> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk_result) = stream.next().await {
        let bytes = chunk_result.map_err(|e| LlmError::NetworkError(e.to_string()))?;
        let text = String::from_utf8_lossy(&bytes);
        buffer.push_str(&text);

        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim().to_string();
            buffer = buffer[newline_pos + 1..].to_string();

            if line.is_empty() || skip_line(&line) {
                continue;
            }

            if let Some(chunk) = parse_line(&line) {
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

/// Check an HTTP response for errors, returning the response if successful.
///
/// `extract_message` is called with the response body text and should try to
/// extract a structured error message (e.g., from JSON). Returns `None` to
/// fall back to the raw body text.
pub async fn check_http_error(
    response: reqwest::Response,
    extract_message: impl FnOnce(&str) -> Option<String>,
) -> Result<reqwest::Response, LlmError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body_text = response
        .text()
        .await
        .unwrap_or_else(|_| "Unknown error".to_string());

    let message = extract_message(&body_text).unwrap_or(body_text);

    Err(LlmError::ApiError {
        status: status.as_u16(),
        message,
    })
}
