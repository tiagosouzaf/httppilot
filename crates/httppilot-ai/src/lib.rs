//! API-only AI providers for HttpPilot.
//!
//! The assistant is opt-in: it receives ONLY text explicitly entered in the
//! AI panel. Requests, API response bodies and workspace data are never added
//! automatically. API credentials live in memory or in the OS keychain.

use std::{fmt, time::Duration};

use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use thiserror::Error;

const MAX_PROMPT_BYTES: usize = 32 * 1024;
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_ERROR_CHARS: usize = 280;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AiProvider {
    #[default]
    OpenAi,
    Anthropic,
}

impl AiProvider {
    pub const ALL: [Self; 2] = [Self::OpenAi, Self::Anthropic];

    pub const fn keychain_account(self) -> &'static str {
        match self {
            Self::OpenAi => "openai-api-key",
            Self::Anthropic => "anthropic-api-key",
        }
    }

    pub const fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-6-luna",
            Self::Anthropic => "claude-haiku-5-5",
        }
    }

    const fn endpoint(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1/responses",
            Self::Anthropic => "https://api.anthropic.com/v1/messages",
        }
    }
}

impl fmt::Display for AiProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OpenAi => "OpenAI (GPT)",
            Self::Anthropic => "Anthropic (Claude)",
        })
    }
}

/// Never derive Debug: an API request includes a secret key.
pub struct AiRequest {
    pub provider: AiProvider,
    pub model: String,
    pub prompt: String,
    pub api_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiAnswer {
    pub content: String,
    pub provider: AiProvider,
    pub model: String,
}

#[derive(Debug, Error)]
pub enum AiError {
    #[error("API key is empty. Enter a key or load it from the OS keychain.")]
    MissingKey,
    #[error("Choose a model available to your API account.")]
    MissingModel,
    #[error("Enter a prompt to send.")]
    MissingPrompt,
    #[error("Prompt exceeds the 32 KiB limit.")]
    PromptTooLong,
    #[error("AI response exceeded the 512 KiB safety limit.")]
    ResponseTooLarge,
    #[error("Provider returned no text.")]
    EmptyAnswer,
    #[error("Provider returned an unexpected response format.")]
    InvalidResponse,
    #[error("Provider HTTP {0}: {1}")]
    HttpStatus(u16, String),
    #[error("Request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Invalid provider JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct AiEngine {
    client: Client,
}

impl AiEngine {
    pub fn new() -> Result<Self, AiError> {
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(90))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    /// Sends user-entered prompt text to a fixed, official provider endpoint.
    pub async fn ask(&self, request: AiRequest) -> Result<AiAnswer, AiError> {
        validate(&request)?;
        let payload = payload_for(&request);
        let builder = self
            .client
            .post(request.provider.endpoint())
            .bearer_auth(request.api_key.trim())
            .json(&payload);
        let builder = match request.provider {
            AiProvider::OpenAi => builder,
            AiProvider::Anthropic => builder.header("anthropic-version", "2026-10-21"),
        };

        let mut response = builder.send().await?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(bytes.len()) {
                return Err(AiError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        let root: Value = serde_json::from_slice(&bytes)?;
        if !status.is_success() {
            return Err(AiError::HttpStatus(status.as_u16(), provider_error(&root, status)));
        }

        let content = parse_answer(request.provider, &root)?;
        Ok(AiAnswer {
            content,
            provider: request.provider,
            model: request.model,
        })
    }
}

fn validate(request: &AiRequest) -> Result<(), AiError> {
    if request.api_key.trim().is_empty() {
        return Err(AiError::MissingKey);
    }
    if request.model.trim().is_empty() {
        return Err(AiError::MissingModel);
    }
    if request.prompt.trim().is_empty() {
        return Err(AiError::MissingPrompt);
    }
    if request.prompt.len() > MAX_PROMPT_BYTES {
        return Err(AiError::PromptTooLong);
    }
    Ok(())
}

fn payload_for(request: &AiRequest) -> Value {
    match request.provider {
        AiProvider::OpenAi => json!({
            "model": request.model.trim(),
            "input": request.prompt,
            "store": false,
            "max_output_tokens": 4096
        }),
        AiProvider::Anthropic => json!({
            "model": request.model.trim(),
            "max_tokens": 2048,
            "messages": [{"role": "user", "content": request.prompt}]
        }),
    }
}

fn parse_answer(provider: AiProvider, root: &Value) -> Result<String, AiError> {
    let texts = match provider {
        AiProvider::OpenAi => root.get("output").and_then(Value::as_array).map(|items| {
            items.iter()
                .filter_map(|item| item.get("content").and_then(Value::as_array))
                .flatten()
                .filter(|content| content.get("type").and_then(Value::as_str) == Some("output_text"))
                .filter_map(|content| content.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
        }),
        AiProvider::Anthropic => root.get("content").and_then(Value::as_array).map(|items| {
            items.iter()
                .filter(|content| content.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|content| content.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
        }),
    };

    let Some(texts) = texts else {
        return Err(AiError::InvalidResponse);
    };
    let answer = texts.join("\n\n");
    if answer.trim().is_empty() {
        Err(AiError::EmptyAnswer)
    } else {
        Ok(answer)
    }
}

fn provider_error(body: &Value, status: StatusCode) -> String {
    let message = body
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("HTTP error"));
    // Do not echo arbitrary provider JSON or response headers (which might
    // include request metadata). Keep error text bounded and user-readable.
    message.chars().take(MAX_ERROR_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(provider: AiProvider) -> AiRequest {
        AiRequest {
            provider,
            model: "test-model".to_owned(),
            prompt: "Show the HTTP response status".to_owned(),
            api_key: "do-not-log-secret".to_owned(),
        }
    }

    #[test]
    fn openai_uses_responses_api_with_storage_disabled() {
        let request = input(AiProvider::OpenAi);
        let json = payload_for(&request);
        assert_eq!(json["store"], false);
        assert_eq!(json["input"], "Show the HTTP response status");
        assert!(json.get("messages").is_none());
        assert_eq!(request.provider.endpoint(), "https://api.openai.com/v1/responses");
        assert_eq!(request.provider.keychain_account(), "openai-api-key");
        assert!(!json.to_string().contains("do-not-log-secret"));
    }

    #[test]
    fn anthropic_uses_messages_api() {
        let request = input(AiProvider::Anthropic);
        let json = payload_for(&request);
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Show the HTTP response status");
        assert_eq!(request.provider.endpoint(), "https://api.anthropic.com/v1/messages");
        assert!(json.get("store").is_none());
        assert!(!json.to_string().contains("do-not-log-secret"));
    }

    #[test]
    fn decodes_openai_multi_part_output_text() {
        let value = json!({"output": [
            {"type": "reasoning", "summary": []},
            {"type": "message", "content": [
                {"type": "output_text", "text": "First part"},
                {"type": "output_text", "text": "Second part"}
            ]}
        ]});
        assert_eq!(
            parse_answer(AiProvider::OpenAi, &value).unwrap(),
            "First part\n\nSecond part"
        );
    }

    #[test]
    fn decodes_anthropic_text_and_ignores_other_blocks() {
        let value = json!({"content": [
            {"type": "thinking", "thinking": "internal"},
            {"type": "text", "text": "Response"}
        ]});
        assert_eq!(parse_answer(AiProvider::Anthropic, &value).unwrap(), "Response");
    }

    #[test]
    fn validates_without_network_request() {
        let mut request = input(AiProvider::OpenAi);
        request.api_key.clear();
        assert!(matches!(validate(&request), Err(AiError::MissingKey)));
        request.api_key = "secret".into();
        request.prompt = "x".repeat(MAX_PROMPT_BYTES + 1);
        assert!(matches!(validate(&request), Err(AiError::PromptTooLong)));
    }

    #[test]
    fn sanitizes_http_errors() {
        let root = json!({"error": {"message": "a".repeat(600)}});
        assert_eq!(provider_error(&root, StatusCode::BAD_REQUEST).len(), 280);
    }
}
