//! Postman Collection v2.1 import/export without cloud services.
//!
//! Postman authentication is intentionally NOT imported or exported: HttpPilot
//! stores authentication separately in the operating system keychain.

use std::collections::VecDeque;

use serde_json::{Value, json};
use thiserror::Error;

use crate::{Collection, Header, QueryParameter, is_sensitive_header};

const POSTMAN_SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

#[derive(Debug, Error)]
pub enum PostmanError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid Postman collection: {0}")]
    Invalid(&'static str),
    #[error("only Postman Collection v2.1 is supported")]
    UnsupportedVersion,
    #[error("unsupported HTTP method in Postman collection: {0}")]
    UnsupportedMethod(String),
    #[error("unsupported Postman body mode: {0}")]
    UnsupportedBodyMode(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedRequest {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub query: Vec<QueryParameter>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedCollection {
    pub name: String,
    pub requests: Vec<ImportedRequest>,
}

fn supported_method(method: &str) -> bool {
    matches!(
        method,
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    )
}

fn query_params(url: &Value) -> Vec<QueryParameter> {
    url.get("query")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| {
            !item
                .get("disabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|item| {
            let name = item.get("key")?.as_str()?.trim();
            if name.is_empty() || is_sensitive_header(name) {
                return None;
            }
            Some(QueryParameter {
                name: name.to_owned(),
                value: item
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            })
        })
        .collect()
}

fn extract_url(url: &Value) -> Result<(String, Vec<QueryParameter>), PostmanError> {
    if let Some(raw) = url.as_str() {
        return Ok((raw.to_owned(), Vec::new()));
    }
    if !url.is_object() {
        return Err(PostmanError::Invalid(
            "request.url must be a string or object",
        ));
    }

    let raw = url.get("raw").and_then(Value::as_str).unwrap_or("");
    // When the raw URL already contains a query string, do not append the
    // structured query list, or the same parameters will be sent twice.
    if !raw.is_empty() {
        let query = if raw.contains('?') {
            Vec::new()
        } else {
            query_params(url)
        };
        return Ok((raw.to_owned(), query));
    }

    let host = url
        .get("host")
        .and_then(Value::as_array)
        .ok_or(PostmanError::Invalid("request.url has no raw URL or host"))?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(".");
    if host.is_empty() {
        return Err(PostmanError::Invalid("request.url host is empty"));
    }
    let protocol = url
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("https");
    let path = url
        .get("path")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let raw = if path.is_empty() {
        format!("{protocol}://{host}")
    } else {
        format!("{protocol}://{host}/{path}")
    };
    Ok((raw, query_params(url)))
}

fn parse_request(item: &Value, name: String) -> Result<ImportedRequest, PostmanError> {
    let request = item
        .get("request")
        .ok_or(PostmanError::Invalid("item is missing request"))?;
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_ascii_uppercase();
    if !supported_method(&method) {
        return Err(PostmanError::UnsupportedMethod(method));
    }

    let url = if request.is_string() {
        request
    } else {
        request
            .get("url")
            .ok_or(PostmanError::Invalid("request is missing url"))?
    };
    let (url, query) = extract_url(url)?;

    let headers = request
        .get("header")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|header| {
            !header
                .get("disabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|header| {
            let name = header.get("key")?.as_str()?.trim();
            if name.is_empty() || is_sensitive_header(name) {
                return None;
            }
            Some(Header {
                name: name.to_owned(),
                value: header
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            })
        })
        .collect();

    let body = if let Some(body) = request.get("body") {
        match body.get("mode").and_then(Value::as_str) {
            None | Some("raw") => body
                .get("raw")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            Some(mode) => return Err(PostmanError::UnsupportedBodyMode(mode.to_owned())),
        }
    } else {
        String::new()
    };

    Ok(ImportedRequest {
        name,
        method,
        url,
        headers,
        query,
        body,
    })
}

/// Parse Postman Collection v2.1; nested folder paths become request-name
/// prefixes because the current HttpPilot workspace is a flat collection tree.
///
/// Imported authorization definitions are deliberately ignored. Known
/// credential-bearing headers and structured query keys are dropped, but
/// arbitrary URLs and request bodies may still contain sensitive information.
pub fn import_postman_json(input: &str) -> Result<ImportedCollection, PostmanError> {
    let document: Value = serde_json::from_str(input)?;
    let info = document
        .get("info")
        .ok_or(PostmanError::Invalid("missing info"))?;
    let name = info
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or(PostmanError::Invalid("info.name is missing"))?
        .to_owned();

    if let Some(schema) = info.get("schema").and_then(Value::as_str)
        && schema != POSTMAN_SCHEMA
    {
        return Err(PostmanError::UnsupportedVersion);
    }

    let items = document
        .get("item")
        .and_then(Value::as_array)
        .ok_or(PostmanError::Invalid("missing item array"))?;

    let mut pending = VecDeque::new();
    for item in items {
        pending.push_back((String::new(), item));
    }
    let mut requests = Vec::new();
    while let Some((prefix, item)) = pending.pop_front() {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Untitled")
            .trim();
        let full_name = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix} / {name}")
        };

        if let Some(children) = item.get("item").and_then(Value::as_array) {
            for child in children {
                pending.push_back((full_name.clone(), child));
            }
        } else {
            requests.push(parse_request(item, full_name)?);
        }
    }
    Ok(ImportedCollection { name, requests })
}

/// Export one local collection to a Postman v2.1 JSON document.
/// Authentication definitions and known sensitive headers are intentionally
/// omitted. URLs, query values and raw bodies are not automatically sanitized.
pub fn export_postman_json(collection: &Collection) -> Result<String, PostmanError> {
    let items = collection
        .requests
        .iter()
        .map(|request| {
            let headers: Vec<Value> = request
                .headers
                .iter()
                .filter(|h| !is_sensitive_header(&h.name))
                .map(|header| json!({"key": header.name, "value": header.value}))
                .collect();
            let query: Vec<Value> = request
                .query
                .iter()
                .filter(|p| !is_sensitive_header(&p.name))
                .map(|param| json!({"key": param.name, "value": param.value}))
                .collect();
            let mut data = json!({
                "method": request.method,
                "header": headers,
                "url": {"raw": request.url, "query": query},
            });
            if !request.body.is_empty() {
                data["body"] = json!({"mode": "raw", "raw": request.body});
            }
            json!({"name": request.name, "request": data})
        })
        .collect::<Vec<_>>();
    let document = json!({
        "info": {"name": collection.name, "schema": POSTMAN_SCHEMA},
        "item": items,
    });
    Ok(serde_json::to_string_pretty(&document)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_nested_folder_and_filters_credentials() {
        let source = json!({
            "info": {"name": "Example", "schema": POSTMAN_SCHEMA},
            "item": [
                {"name": "Users", "item": [
                    {"name": "Get one", "request": {
                        "method": "GET",
                        "url": {"raw": "https://api.test/users", "query": [
                            {"key": "search", "value": "hello world"},
                            {"key": "x-api-key", "value": "do-not-store"}]},
                        "header": [
                            {"key": "Accept", "value": "application/json"},
                            {"key": "Authorization", "value": "do-not-store"}],
                        "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "secret"}]}
                    }}
                ]}
            ]
        });
        let imported = import_postman_json(&source.to_string()).unwrap();
        assert_eq!(imported.name, "Example");
        assert_eq!(imported.requests[0].name, "Users / Get one");
        assert_eq!(imported.requests[0].query[0].name, "search");
        assert_eq!(imported.requests[0].query.len(), 1);
        assert_eq!(imported.requests[0].headers.len(), 1);
        let debug = format!("{imported:?}");
        assert!(!debug.contains("do-not-store"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn preserves_raw_query_without_readding_structured_fields() {
        let source = json!({
            "info": {"name": "A"},
            "item": [{"name": "A", "request": {
                "method": "GET",
                "url": {"raw": "https://example.org/?q=hello", "query": [
                    {"key": "q", "value": "hello"}]}}}]
        });
        let imported = import_postman_json(&source.to_string()).unwrap();
        assert_eq!(imported.requests[0].url, "https://example.org/?q=hello");
        assert!(imported.requests[0].query.is_empty());
    }

    #[test]
    fn export_round_trip_request_details() {
        let collection = Collection {
            id: 8,
            name: "Public endpoints".into(),
            requests: vec![crate::SavedRequest {
                id: 9,
                name: "Create".into(),
                method: "POST".into(),
                url: "https://example.org/people".into(),
                headers: vec![
                    Header {
                        name: "Accept".into(),
                        value: "application/json".into(),
                    },
                    Header {
                        name: "Authorization".into(),
                        value: "private".into(),
                    },
                ],
                query: vec![QueryParameter {
                    name: "limit".into(),
                    value: "3".into(),
                }],
                body: r#"{"name":"demo"}"#.into(),
            }],
        };
        let exported = export_postman_json(&collection).unwrap();
        assert!(!exported.contains("private"));
        let imported = import_postman_json(&exported).unwrap();
        assert_eq!(imported.requests.len(), 1);
        assert_eq!(imported.requests[0].query[0].value, "3");
        assert_eq!(imported.requests[0].body, r#"{"name":"demo"}"#);
        assert_eq!(imported.requests[0].headers.len(), 1);
    }

    #[test]
    fn rejects_unsupported_body_mode_without_losing_data() {
        let source = json!({
            "info": {"name": "A"},
            "item": [{"name": "Upload", "request": {
                "method": "POST", "url": "https://example.org",
                "body": {"mode": "formdata", "formdata": []}}}]
        });
        assert!(matches!(
            import_postman_json(&source.to_string()),
            Err(PostmanError::UnsupportedBodyMode(_))
        ));
    }

    #[test]
    fn refuses_v2_0_collection() {
        let source = json!({
            "info": {
                "name": "Legacy",
                "schema": "https://schema.getpostman.com/json/collection/v2.0.0/collection.json"
            },
            "item": []
        });
        assert!(matches!(
            import_postman_json(&source.to_string()),
            Err(PostmanError::UnsupportedVersion)
        ));
    }
}
