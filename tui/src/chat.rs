use futures::StreamExt;
use tokio::sync::mpsc;

use crate::client::SmgClient;

/// A chat message in the playground conversation.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Which API endpoint to use for chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChatEndpoint {
    #[default]
    Chat,
    Responses,
}

impl ChatEndpoint {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Responses => "responses",
        }
    }

    pub fn cycle(&self) -> Self {
        match self {
            Self::Chat => Self::Responses,
            Self::Responses => Self::Chat,
        }
    }
}

/// Stream a chat completion from the SMG gateway, sending tokens via `tx`.
/// Sends "\n[DONE]" when complete, "\n[ERROR]..." on failure.
pub async fn stream_chat(
    client: &SmgClient,
    model: &str,
    messages: &[serde_json::Value],
    endpoint: ChatEndpoint,
    tx: mpsc::UnboundedSender<String>,
) {
    match endpoint {
        ChatEndpoint::Chat => stream_chat_completions(client, model, messages, tx).await,
        ChatEndpoint::Responses => stream_responses(client, model, messages, tx).await,
    }
}

async fn stream_chat_completions(
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

    let resp = match client.stream_request("/v1/chat/completions", &body).await {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(format!("\n[ERROR]{e}"));
            return;
        }
    };

    process_sse_stream(resp, "choices", tx).await;
}

async fn stream_responses(
    client: &SmgClient,
    model: &str,
    messages: &[serde_json::Value],
    tx: mpsc::UnboundedSender<String>,
) {
    // Convert chat messages to responses API input format
    let input: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": m["role"],
                "content": m["content"],
            })
        })
        .collect();

    let body = serde_json::json!({
        "model": model,
        "input": input,
        "stream": true,
    });

    let resp = match client.stream_request("/v1/responses", &body).await {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(format!("\n[ERROR]{e}"));
            return;
        }
    };

    // Responses API uses a different streaming format
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
                    // Responses API sends different event types
                    let event_type = parsed["type"].as_str().unwrap_or("");
                    match event_type {
                        "response.output_text.delta" => {
                            if let Some(delta) = parsed["delta"].as_str() {
                                let _ = tx.send(delta.to_string());
                            }
                        }
                        "response.completed" | "response.done" => {
                            let _ = tx.send("\n[DONE]".to_string());
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let _ = tx.send("\n[DONE]".to_string());
}

async fn process_sse_stream(
    resp: reqwest::Response,
    _format: &str,
    tx: mpsc::UnboundedSender<String>,
) {
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

    let _ = tx.send("\n[DONE]".to_string());
}
