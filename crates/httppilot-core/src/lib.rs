//! Shared, asynchronous HTTP engine for HttpPilot.
//! The UI and CLI use the same engine, with bounded response previews.

use std::time::{Duration, Instant};

use reqwest::{Client, Method, header::CONTENT_TYPE};
use thiserror::Error;

/// Maximum amount of response data kept in memory for a preview.
pub const DEFAULT_PREVIEW_LIMIT: usize = 1024 * 1024;

/// One HTTP request, independent of any user interface.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl HttpRequest {
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }
}

/// Request result. `body_preview` is limited, even for very large responses.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub elapsed: Duration,
    pub content_type: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body_preview: String,
    pub preview_bytes: usize,
    pub truncated: bool,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("invalid HTTP method: {0}")]
    InvalidMethod(String),

    #[error(transparent)]
    Request(#[from] reqwest::Error),
}

/// Cheap to clone: Reqwest shares its underlying connection pool.
#[derive(Debug, Clone)]
pub struct HttpEngine {
    client: Client,
    preview_limit: usize,
}

impl Default for HttpEngine {
    fn default() -> Self {
        Self::new(DEFAULT_PREVIEW_LIMIT)
    }
}

impl HttpEngine {
    pub fn new(preview_limit: usize) -> Self {
        Self {
            client: Client::new(),
            preview_limit,
        }
    }

    /// Execute an HTTP request with a 30-second timeout.
    ///
    /// Large bodies are read in chunks. Reading stops once the preview budget
    /// is reached, rather than buffering the entire body in the application.
    pub async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, EngineError> {
        let method = Method::from_bytes(request.method.as_bytes())
            .map_err(|_| EngineError::InvalidMethod(request.method.clone()))?;

        let mut builder = self
            .client
            .request(method, &request.url)
            .timeout(Duration::from_secs(30));

        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }

        let started = Instant::now();
        let mut response = builder.send().await?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let headers = response
            .headers()
            .iter()
            .map(|(key, value)| {
                (key.to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned())
            })
            .collect();

        let mut preview = Vec::new();
        let mut truncated = false;

        while let Some(chunk) = response.chunk().await? {
            let remaining = self.preview_limit.saturating_sub(preview.len());
            if chunk.len() > remaining {
                preview.extend_from_slice(&chunk[..remaining]);
                truncated = true;
                break;
            }
            preview.extend_from_slice(&chunk);
        }

        let elapsed = started.elapsed();
        let preview_bytes = preview.len();

        Ok(HttpResponse {
            status,
            elapsed,
            content_type,
            headers,
            body_preview: String::from_utf8_lossy(&preview).into_owned(),
            preview_bytes,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_get_request() {
        let request = HttpRequest::new("GET", "https://example.com");
        assert_eq!(request.method, "GET");
        assert!(request.body.is_none());
        assert!(request.headers.is_empty());
    }

    #[tokio::test]
    async fn rejects_invalid_method_before_network_io() {
        let error = HttpEngine::default()
            .execute(HttpRequest::new("GET BAD", "https://example.com"))
            .await
            .unwrap_err();
        assert!(matches!(error, EngineError::InvalidMethod(_)));
    }
}
