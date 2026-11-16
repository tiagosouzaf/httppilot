//! Import an OpenAPI 3.x JSON document into a local HttpPilot collection.
//!
//! The converter is entirely offline: no remote $ref fetching, no API calls,
//! no implicit authentication values, and no embedded request body examples.
//!
//! Request path/query/header variables are represented as {{name}} templates
//! that the user must explicitly define in an HttpPilot environment.

use serde_json::Value;
use thiserror::Error;

use crate::{Header, ImportedCollection, ImportedRequest, QueryParameter, is_sensitive_header};

const MAX_OPERATIONS: usize = 3000;

#[derive(Debug, Error)]
pub enum OpenApiError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid OpenAPI document: {0}")]
    Invalid(&'static str),
    #[error("only OpenAPI 3.0, 3.1 and 3.2 JSON documents are supported")]
    UnsupportedVersion,
    #[error("external or unresolved OpenAPI $ref is unsupported: {0}")]
    UnsupportedReference(String),
    #[error("unsupported HTTP operation: {0}")]
    UnsupportedMethod(String),
    #[error("invalid or unsupported API server URL: {0}")]
    UnsupportedServer(String),
    #[error("unsupported OpenAPI request body media type: {0}")]
    UnsupportedBody(String),
    #[error("OpenAPI document exceeds 3000 operations")]
    TooManyOperations,
}

fn resolve<'a>(root: &'a Value, value: &'a Value) -> Result<&'a Value, OpenApiError> {
    let Some(reference) = value.get("$ref").and_then(Value::as_str) else {
        return Ok(value);
    };
    if !reference.starts_with("#/components/") {
        return Err(OpenApiError::UnsupportedReference(reference.to_owned()));
    }
    let resolved = root
        .pointer(&reference[1..])
        .ok_or_else(|| OpenApiError::UnsupportedReference(reference.to_owned()))?;
    if resolved.get("$ref").is_some() {
        return Err(OpenApiError::UnsupportedReference(reference.to_owned()));
    }
    Ok(resolved)
}

fn first_server(source: &Value) -> Option<&Value> {
    source
        .get("servers")
        .and_then(Value::as_array)
        .and_then(|servers| servers.first())
}

fn server_url(root: &Value, path: &Value, operation: &Value) -> Result<String, OpenApiError> {
    let selected = first_server(operation)
        .or_else(|| first_server(path))
        .or_else(|| first_server(root));

    let Some(server) = selected else {
        // OpenAPI defaults to "/" in the absence of servers. Preserve the
        // unresolved host as an explicit HttpPilot environment variable.
        return Ok("{{baseUrl}}".to_owned());
    };
    let url = server
        .get("url")
        .and_then(Value::as_str)
        .ok_or(OpenApiError::Invalid("servers[0].url is required"))?;

    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(OpenApiError::UnsupportedServer(url.to_owned()));
    }
    // OAS server variables are deliberately NOT replaced with their defaults
    // because defaults may be credentials or other sensitive values.
    wrap_path_variables(url)
}

fn wrap_path_variables(input: &str) -> Result<String, OpenApiError> {
    let mut output = String::with_capacity(input.len() + 8);
    let mut rest = input;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let unclosed = &rest[start + 1..];
        let Some(end) = unclosed.find('}') else {
            return Err(OpenApiError::Invalid("unclosed OpenAPI path variable"));
        };
        let variable = &unclosed[..end];
        if variable.is_empty() || variable.contains('{') {
            return Err(OpenApiError::Invalid("invalid OpenAPI path variable"));
        }
        output.push_str("{{");
        output.push_str(variable);
        output.push_str("}}");
        rest = &unclosed[end + 1..];
    }
    if rest.contains('}') {
        return Err(OpenApiError::Invalid("unmatched OpenAPI path variable"));
    }
    output.push_str(rest);
    Ok(output)
}

fn fill_parameters(
    root: &Value,
    path_item: &Value,
    operation: &Value,
    headers: &mut Vec<Header>,
    query: &mut Vec<QueryParameter>,
) -> Result<(), OpenApiError> {
    let mut params: Vec<(String, String, Value)> = Vec::new();
    for source in [path_item, operation] {
        if let Some(items) = source.get("parameters").and_then(Value::as_array) {
            for raw in items {
                let parameter = resolve(root, raw)?;
                let name = parameter
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(OpenApiError::Invalid("parameter name is missing"))?;
                let location = parameter
                    .get("in")
                    .and_then(Value::as_str)
                    .ok_or(OpenApiError::Invalid("parameter location is missing"))?;
                if let Some(index) = params
                    .iter()
                    .position(|(n, loc, _)| n == name && loc == location)
                {
                    params.remove(index);
                }
                params.push((name.to_owned(), location.to_owned(), parameter.clone()));
            }
        }
    }

    for (name, location, _) in params {
        if name.trim().is_empty() || is_sensitive_header(&name) {
            continue;
        }
        match location.as_str() {
            "query" => query.push(QueryParameter {
                value: format!("{{{{{name}}}}}"),
                name,
            }),
            "header" => headers.push(Header {
                value: format!("{{{{{name}}}}}"),
                name,
            }),
            "path" | "cookie" => {
                // Path templates are already present in the imported URL.
                // Cookie values are not imported or stored.
            }
            _ => return Err(OpenApiError::Invalid("unsupported parameter location")),
        }
    }
    Ok(())
}

fn request_body(
    root: &Value,
    operation: &Value,
    headers: &mut Vec<Header>,
) -> Result<(), OpenApiError> {
    let Some(raw) = operation.get("requestBody") else {
        return Ok(());
    };
    let body = resolve(root, raw)?;
    let Some(content) = body.get("content").and_then(Value::as_object) else {
        return Ok(());
    };
    if content.is_empty() {
        return Ok(());
    }
    let media = if content.contains_key("application/json") {
        "application/json"
    } else if content.contains_key("text/plain") {
        "text/plain"
    } else if content.contains_key("application/xml") {
        "application/xml"
    } else {
        return Err(OpenApiError::UnsupportedBody(
            content.keys().next().cloned().unwrap_or_default(),
        ));
    };
    // Do not import schema examples/defaults: they may contain credentials.
    headers.push(Header {
        name: "Content-Type".to_owned(),
        value: media.to_owned(),
    });
    Ok(())
}

/// Convert an OpenAPI 3.x JSON document to a collection without contacting its
/// server. Missing hosts and parameter values remain {{variable}} placeholders.
///
/// Not imported: authentication/security schemes, cookies, remote references,
/// request examples, binary/multipart bodies, callbacks, webhooks and scripts.
pub fn import_openapi_json(input: &str) -> Result<ImportedCollection, OpenApiError> {
    let root: Value = serde_json::from_str(input)?;
    let version = root.get("openapi").and_then(Value::as_str).unwrap_or("");
    if !(version.starts_with("3.0.") || version.starts_with("3.1.") || version.starts_with("3.2."))
    {
        return Err(OpenApiError::UnsupportedVersion);
    }
    let name = root
        .pointer("/info/title")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(OpenApiError::Invalid("info.title is required"))?
        .trim()
        .to_owned();
    let paths = root
        .get("paths")
        .and_then(Value::as_object)
        .ok_or(OpenApiError::Invalid("paths object is required"))?;

    let mut requests = Vec::new();
    for (path, raw_item) in paths {
        if !path.starts_with('/') {
            return Err(OpenApiError::Invalid("path names must begin with /"));
        }
        let path_item = resolve(&root, raw_item)?;
        for (method, verb) in [
            ("get", "GET"),
            ("post", "POST"),
            ("put", "PUT"),
            ("patch", "PATCH"),
            ("delete", "DELETE"),
            ("head", "HEAD"),
            ("options", "OPTIONS"),
            ("trace", "TRACE"),
        ] {
            let Some(operation) = path_item.get(method) else {
                continue;
            };
            if method == "trace" {
                return Err(OpenApiError::UnsupportedMethod("TRACE".to_owned()));
            }
            if requests.len() >= MAX_OPERATIONS {
                return Err(OpenApiError::TooManyOperations);
            }
            let operation = resolve(&root, operation)?;
            let host = server_url(&root, path_item, operation)?;
            let uri = format!(
                "{}{}",
                host.trim_end_matches('/'),
                wrap_path_variables(path)?
            );
            let summary = operation
                .get("summary")
                .and_then(Value::as_str)
                .or_else(|| operation.get("operationId").and_then(Value::as_str))
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(path);
            let prefix = operation
                .get("tags")
                .and_then(Value::as_array)
                .and_then(|tags| tags.first())
                .and_then(Value::as_str);
            let name = match prefix {
                Some(tag) if !tag.is_empty() => format!("{tag} / {summary}"),
                _ => format!("{verb} {summary}"),
            };

            let mut headers = Vec::new();
            let mut query = Vec::new();
            fill_parameters(&root, path_item, operation, &mut headers, &mut query)?;
            request_body(&root, operation, &mut headers)?;
            requests.push(ImportedRequest {
                name,
                method: verb.to_owned(),
                url: uri,
                headers,
                query,
                body: String::new(),
            });
        }
    }

    Ok(ImportedCollection { name, requests })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn imports_paths_operation_overrides_and_parameter_refs() {
        let source = json!({
            "openapi": "3.1.0",
            "info": {"title": "Pet API", "version": "1"},
            "servers": [{"url": "https://api.example.com/v1"}],
            "components": {"parameters": {
                "limit": {"name": "limit", "in": "query", "schema": {"default": "SECRET-DEFAULT"}},
                "apiKey": {"name": "x-api-key", "in": "header"}
            }},
            "paths": {"/pets/{id}": {
                "parameters": [{"$ref": "#/components/parameters/limit"}],
                "get": {
                    "summary": "Get pet", "tags": ["Pets"],
                    "parameters": [
                        {"name": "id", "in": "path", "required": true},
                        {"$ref": "#/components/parameters/apiKey"},
                        {"name": "limit", "in": "query", "schema": {"example": "SECRET-EXAMPLE"}}
                    ],
                    "security": [{"Bearer": []}]
                }
            }}
        });
        let collection = import_openapi_json(&source.to_string()).unwrap();
        assert_eq!(collection.name, "Pet API");
        assert_eq!(collection.requests.len(), 1);
        let request = &collection.requests[0];
        assert_eq!(request.name, "Pets / Get pet");
        assert_eq!(request.method, "GET");
        assert_eq!(request.url, "https://api.example.com/v1/pets/{{id}}");
        assert_eq!(
            request.query,
            vec![QueryParameter {
                name: "limit".into(),
                value: "{{limit}}".into()
            }]
        );
        assert!(request.headers.is_empty());
        let snapshot = format!("{collection:?}");
        assert!(!snapshot.contains("SECRET"));
        assert!(!snapshot.contains("Bearer"));
    }

    #[test]
    fn chooses_operation_server_and_preserves_host_template() {
        let source = json!({
            "openapi": "3.0.3", "info": {"title": "Users"},
            "servers": [{"url": "https://root.example.com"}],
            "paths": {"/people": {"post": {
                "summary": "Create",
                "servers": [{"url": "https://{region}.example.com/api", "variables": {
                    "region": {"default": "private-internal"}
                }}],
                "requestBody": {"content": {"application/json": {
                    "example": {"apiKey": "SECRET"}
                }}}
            }}}
        });
        let imported = import_openapi_json(&source.to_string()).unwrap();
        let req = &imported.requests[0];
        assert_eq!(req.url, "https://{{region}}.example.com/api/people");
        assert_eq!(
            req.headers,
            vec![Header {
                name: "Content-Type".into(),
                value: "application/json".into()
            }]
        );
        assert!(req.body.is_empty());
        assert!(!format!("{imported:?}").contains("SECRET"));
        assert!(!format!("{imported:?}").contains("private-internal"));
    }

    #[test]
    fn no_server_requires_explicit_environment_variable() {
        let source = json!({"openapi":"3.2.0","info":{"title":"API"},"paths":{
            "/users": {"get": {"summary": "List users"}}
        }});
        let imported = import_openapi_json(&source.to_string()).unwrap();
        assert_eq!(imported.requests[0].url, "{{baseUrl}}/users");
    }

    #[test]
    fn rejects_relative_servers_without_guessing_host() {
        let source = json!({"openapi":"3.1.1","info":{"title":"API"},
            "servers":[{"url":"/v1"}],"paths":{"/users":{"get":{}}}});
        assert!(matches!(
            import_openapi_json(&source.to_string()),
            Err(OpenApiError::UnsupportedServer(_))
        ));
    }

    #[test]
    fn rejects_external_refs_binary_bodies_and_trace_without_partial_import() {
        let base = json!({"openapi":"3.1.1","info":{"title":"API"},"paths":{}});
        for operation in [
            json!({"parameters":[{"$ref":"https://host.invalid/params.json"}]}),
            json!({"requestBody":{"content":{"multipart/form-data":{}}}}),
        ] {
            let mut doc = base.clone();
            doc["paths"]["/users"]["post"] = operation;
            assert!(import_openapi_json(&doc.to_string()).is_err());
        }
        let mut doc = base.clone();
        doc["paths"]["/users"]["trace"] = json!({});
        assert!(matches!(
            import_openapi_json(&doc.to_string()),
            Err(OpenApiError::UnsupportedMethod(_))
        ));
    }

    #[test]
    fn rejects_unknown_openapi_versions() {
        let source = json!({"swagger":"2.0","info":{"title":"Legacy"},"paths":{}});
        assert!(matches!(
            import_openapi_json(&source.to_string()),
            Err(OpenApiError::UnsupportedVersion)
        ));
    }
}
