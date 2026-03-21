use futures::StreamExt;
use tokio::sync::mpsc;

use crate::client::SmgClient;

/// A chat message in the playground conversation.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Stream a chat completion from the SMG gateway, sending tokens via `tx`.
/// Sends "\n[DONE]" when complete, "\n[ERROR]..." on failure.
pub async fn stream_chat(
    client: &SmgClient,
    model: &str,
    messages: &[serde_json::Value],
    tx: mpsc::UnboundedSender<String>,
) {
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });

    let resp = match client
        .stream_request("/v1/chat/completions", &body)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(format!("\n[ERROR]{e}"));
            return;
        }
    };

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(format!("\n[ERROR]{e}"));
                return;
            }
        };

        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete SSE lines
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].to_string();
            buffer = buffer[pos + 1..].to_string();

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if line == "data: [DONE]" {
                let _ = tx.send("\n[DONE]".to_string());
                return;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str() {
                        let _ = tx.send(delta.to_string());
                    }
                }
            }
        }
    }

    // Stream ended without [DONE]
    let _ = tx.send("\n[DONE]".to_string());
}
