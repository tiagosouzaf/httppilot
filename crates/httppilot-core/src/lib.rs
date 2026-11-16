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
    pub query: Vec<(String, String)>,
    pub auth: RequestAuth,
    pub body: Option<String>,
}

impl HttpRequest {
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: Vec::new(),
            query: Vec::new(),
            auth: RequestAuth::None,
            body: None,
        }
    }
}

/// Authentication is held only in memory and is never part of a saved request.
#[derive(Debug, Clone, Default)]
pub enum RequestAuth {
    #[default]
    None,
    Bearer(String),
    Basic {
        username: String,
        password: String,
    },
}

/// Replace single-pass {{variable}} placeholders without recursively expanding values.
/// Missing variables are errors instead of silently sending an incorrect request.
pub fn render_template(input: &str, vars: &[(String, String)]) -> Result<String, TemplateError> {
    let mut output = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(start) = remaining.find("{{") {
        output.push_str(&remaining[..start]);
        let open = &remaining[start + 2..];
        let Some(end) = open.find("}}") else {
            return Err(TemplateError::UnclosedPlaceholder);
        };
        let name = open[..end].trim();
        let value = vars
            .iter()
            .find(|(key, _)| key == name)
            .ok_or_else(|| TemplateError::MissingVariable(name.to_owned()))?;
        output.push_str(&value.1);
        remaining = &open[end + 2..];
    }
    output.push_str(remaining);
    Ok(output)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TemplateError {
    #[error("missing environment variable: {0}")]
    MissingVariable(String),
    #[error("unclosed {{variable}} placeholder")]
    UnclosedPlaceholder,
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
        if !request.query.is_empty() {
            builder = builder.query(&request.query);
        }
        builder = match request.auth {
            RequestAuth::None => builder,
            RequestAuth::Bearer(token) => builder.bearer_auth(token),
            RequestAuth::Basic { username, password } => {
                builder.basic_auth(username, Some(password))
            }
        };
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
                (
                    key.to_string(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
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

    #[test]
    fn expands_templates_once_and_rejects_missing_variables() {
        let vars = vec![(String::from("baseUrl"), String::from("https://example.org"))];
        assert_eq!(
            render_template("{{baseUrl}}/v1", &vars).unwrap(),
            "https://example.org/v1"
        );
        assert_eq!(
            render_template("{{missing}}", &vars),
            Err(TemplateError::MissingVariable("missing".to_owned()))
        );
        assert_eq!(
            render_template("{{oops", &vars),
            Err(TemplateError::UnclosedPlaceholder)
        );
    }

    #[test]
    fn values_are_not_recursively_interpolated() {
        let vars = vec![(String::from("a"), String::from("{{b}}"))];
        assert_eq!(render_template("{{a}}", &vars).unwrap(), "{{b}}");
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
