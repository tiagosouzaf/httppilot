//! Official local CLI integration. No token scraping or proxying of Claude
//! subscription credentials: Claude Code is launched as its own application.
//!
//! Codex subscription mode invokes the authenticated official CLI using a
//! read-only sandbox. Explicit opt-in enables the CLI's existing MCP config.
//! MCP tools may have remote side effects outside of the CLI filesystem sandbox.

use std::{io, process::Stdio, time::Duration};

use serde_json::Value;
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_ANSWER_BYTES: usize = 512 * 1024;
const MAX_MCP_SERVERS: usize = 64;
const MAX_PROMPT_BYTES: usize = 32 * 1024;

#[derive(Debug, Error)]
pub enum LocalCliError {
    #[error(
        "Codex CLI is not installed or not available on PATH. Install the official CLI and run: codex login"
    )]
    CodexUnavailable,
    #[error("Codex CLI is not signed in to ChatGPT. Run: codex login")]
    SignInRequired,
    #[error(
        "Codex CLI is authenticated with an API key, not a ChatGPT subscription. Run: codex logout && codex login"
    )]
    NotSubscription,
    #[error("Could not confirm the Codex authentication method. Run: codex login status")]
    UnknownAuthentication,
    #[error("Enter a prompt before sending")]
    EmptyPrompt,
    #[error("Prompt exceeds 32 KiB")]
    PromptTooLong,
    #[error("Local CLI returned an error; check Codex authentication, network and permissions")]
    CodexFailed,
    #[error("Codex did not finish within two minutes")]
    CodexTimedOut,
    #[error("Codex output was empty")]
    EmptyAnswer,
    #[error("Codex output exceeded the 512 KiB limit")]
    OutputTooLarge,
    #[error("Local MCP configuration could not be read")]
    McpUnavailable,
    #[error("Selected MCP server is not configured locally: {0}")]
    McpNotFound(String),
    #[error("MCP server has a name that cannot be safely overridden: {0}")]
    InvalidMcpName(String),
    #[error("Too many local MCP servers are configured (limit: 64)")]
    TooManyMcpServers,
    #[error("Codex produced an unexpected response format")]
    InvalidCodexOutput,
    #[error("Failed to run official Claude Code in a terminal. Open a terminal and run: claude")]
    ClaudeCodeUnavailable,
    #[error("Local process error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexAuth {
    ChatGptSubscription,
    ApiKey,
    NotSignedIn,
    Unknown,
}

fn classify_login_status(success: bool, raw: &str) -> CodexAuth {
    if !success {
        return CodexAuth::NotSignedIn;
    }
    let lower = raw.to_ascii_lowercase();
    if lower.contains("api key") || lower.contains("api_key") {
        CodexAuth::ApiKey
    } else if lower.contains("logged in using chatgpt") || lower.contains("logged in with chatgpt")
    {
        CodexAuth::ChatGptSubscription
    } else {
        CodexAuth::Unknown
    }
}

/// Query the authenticated official Codex binary; never read its auth files.
pub async fn codex_auth_status() -> Result<CodexAuth, LocalCliError> {
    let output = Command::new("codex")
        .args(["login", "status"])
        .output()
        .await
        .map_err(map_codex_spawn)?;
    let text = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(classify_login_status(
        output.status.success(),
        &format!("{text}\n{stderr}"),
    ))
}

/// Execute the official Codex CLI through a saved ChatGPT login only.
///
/// Prompts go through stdin (never as arguments), and only the final agent
/// message from structured JSON events is shown. No raw MCP/tool events,
/// command output or private configuration is displayed.
///
/// Selected MCP servers are explicit opt-in. We query the installed local
/// Codex MCP servers on each call and disable every unselected configured server
/// via invocation-scoped config overrides. No local config files are modified.
///
/// NOTE: MCPs may have remote side effects even in read-only FS sandboxes.
pub async fn ask_codex_subscription(
    prompt: &str,
    selected_mcp: &[String],
) -> Result<String, LocalCliError> {
    if prompt.trim().is_empty() {
        return Err(LocalCliError::EmptyPrompt);
    }
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err(LocalCliError::PromptTooLong);
    }
    match codex_auth_status().await? {
        CodexAuth::ChatGptSubscription => {}
        CodexAuth::ApiKey => return Err(LocalCliError::NotSubscription),
        CodexAuth::NotSignedIn => return Err(LocalCliError::SignInRequired),
        CodexAuth::Unknown => return Err(LocalCliError::UnknownAuthentication),
    }

    let overrides = if selected_mcp.is_empty() {
        Vec::new()
    } else {
        let available = list_codex_mcp_names().await?;
        build_mcp_overrides(&available, selected_mcp)?
    };

    let mut command = Command::new("codex");
    command.args([
        "exec",
        "--json",
        "--ephemeral",
        "--sandbox",
        "read-only",
        "--skip-git-repo-check",
    ]);
    if selected_mcp.is_empty() {
        // Disables the user's Codex config, including registered MCP servers.
        // Login remains available through CODEX_HOME.
        command.arg("--ignore-user-config");
    } else {
        for value in overrides {
            command.args(["-c", &value]);
        }
    }
    command.arg("-");
    command
        .current_dir(std::env::temp_dir())
        .env_remove("CODEX_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let child = command.spawn().map_err(map_codex_spawn)?;
    let result = tokio::time::timeout(Duration::from_secs(120), async move {
        let mut child = child;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes()).await?;
            stdin.flush().await?;
            drop(stdin);
        }

        let output = child
            .stdout
            .take()
            .ok_or(LocalCliError::InvalidCodexOutput)?;
        let mut reader = output.take((MAX_OUTPUT_BYTES as u64) + 1);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        if bytes.len() > MAX_OUTPUT_BYTES {
            // The child is killed on drop rather than buffering unbounded
            // tool results in memory.
            return Err(LocalCliError::OutputTooLarge);
        }

        let status = child.wait().await?;
        if !status.success() {
            return Err(LocalCliError::CodexFailed);
        }
        parse_codex_final_answer(&bytes)
    })
    .await
    .map_err(|_| LocalCliError::CodexTimedOut)??;
    Ok(result)
}

/// Construct safe, invocation-only MCP enable overrides, never editing the
/// user's Codex configuration. Unknown and malformed server names fail closed.
fn build_mcp_overrides(
    available: &[String],
    selected: &[String],
) -> Result<Vec<String>, LocalCliError> {
    if available.len() > MAX_MCP_SERVERS {
        return Err(LocalCliError::TooManyMcpServers);
    }
    for server in selected {
        if !available.contains(server) {
            return Err(LocalCliError::McpNotFound(server.clone()));
        }
    }
    available
        .iter()
        .map(|name| {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
            {
                return Err(LocalCliError::InvalidMcpName(name.clone()));
            }
            // TOML quoted path component prevents a dotted server name from
            // being interpreted as multiple config path components.
            let enabled = selected.contains(name);
            Ok(format!("mcp_servers.\"{name}\".enabled={enabled}"))
        })
        .collect()
}

/// Extract only the last completed assistant message from the CLI's NDJSON
/// stream; raw logs and tool results may contain secrets and are never shown.
fn parse_codex_final_answer(events: &[u8]) -> Result<String, LocalCliError> {
    let mut answer = None;
    for line in events
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
    {
        let event: Value =
            serde_json::from_slice(line).map_err(|_| LocalCliError::InvalidCodexOutput)?;
        if event.get("type").and_then(Value::as_str) == Some("item.completed")
            && event.pointer("/item/type").and_then(Value::as_str) == Some("agent_message")
            && let Some(text) = event.pointer("/item/text").and_then(Value::as_str)
        {
            if text.len() > MAX_ANSWER_BYTES {
                return Err(LocalCliError::OutputTooLarge);
            }
            answer = Some(text.to_owned());
        }
    }
    let content = answer.ok_or(LocalCliError::EmptyAnswer)?;
    if content.trim().is_empty() {
        Err(LocalCliError::EmptyAnswer)
    } else {
        Ok(content)
    }
}

fn map_codex_spawn(error: io::Error) -> LocalCliError {
    if error.kind() == io::ErrorKind::NotFound {
        LocalCliError::CodexUnavailable
    } else {
        LocalCliError::Io(error)
    }
}

/// Show only the names of servers configured in Codex on the local machine.
/// Do not echo raw config, env vars, launch arguments or credentials.
pub async fn list_codex_mcp_names() -> Result<Vec<String>, LocalCliError> {
    let output = Command::new("codex")
        .args(["mcp", "list", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(map_codex_spawn)?;
    if !output.status.success() || output.stdout.len() > MAX_OUTPUT_BYTES {
        return Err(LocalCliError::McpUnavailable);
    }
    let value: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| LocalCliError::McpUnavailable)?;
    Ok(extract_mcp_names(&value))
}

fn extract_mcp_names(config: &Value) -> Vec<String> {
    let mut names = Vec::new();
    let arr = config
        .as_array()
        .or_else(|| config.get("servers").and_then(Value::as_array));

    if let Some(items) = arr {
        for item in items {
            if let Some(name) = item.get("name").and_then(Value::as_str) {
                names.push(name.to_owned());
            }
        }
    } else if let Some(object) = config
        .get("mcp_servers")
        .or_else(|| config.get("servers"))
        .and_then(Value::as_object)
    {
        names.extend(object.keys().cloned());
    }
    names.sort_unstable();
    names.dedup();
    names
}

/// Open the *unmodified* official Claude Code application in the user's
/// terminal. HttpPilot neither receives its response nor processes Claude.ai
/// subscription authentication or tokens.
pub fn launch_official_claude_code() -> Result<(), LocalCliError> {
    #[cfg(target_os = "windows")]
    {
        // Fixed arguments only; no user-controlled shell input.
        std::process::Command::new("cmd.exe")
            .args(["/C", "start", "", "cmd.exe", "/K", "claude"])
            .spawn()
            .map_err(|_| LocalCliError::ClaudeCodeUnavailable)?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let options: &[(&str, &[&str])] = &[
            ("ghostty", &["-e", "claude"]),
            ("kitty", &["-e", "claude"]),
            ("foot", &["-e", "claude"]),
            ("alacritty", &["-e", "claude"]),
            ("x-terminal-emulator", &["-e", "claude"]),
            ("konsole", &["-e", "claude"]),
            ("gnome-terminal", &["--", "claude"]),
        ];
        for (exe, args) in options {
            if std::process::Command::new(exe).args(*args).spawn().is_ok() {
                return Ok(());
            }
        }
        Err(LocalCliError::ClaudeCodeUnavailable)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err(LocalCliError::ClaudeCodeUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filters_codex_events_and_returns_only_final_message() {
        let events = [
            r#"{"type":"item.completed","item":{"type":"command_execution","output":"DO-NOT-SHOW"}}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"Resposta segura"}}"#,
            r#"{"type":"turn.completed","usage":{"total_tokens":12}}"#,
        ]
        .join("\n");
        let answer = parse_codex_final_answer(events.as_bytes()).unwrap();
        assert_eq!(answer, "Resposta segura");
        assert!(!answer.contains("DO-NOT-SHOW"));
    }

    #[test]
    fn malformed_or_empty_codex_output_is_not_displayed() {
        assert!(matches!(
            parse_codex_final_answer(br#"{"type":"item.completed"}"#),
            Err(LocalCliError::EmptyAnswer)
        ));
        assert!(matches!(
            parse_codex_final_answer(b"not json"),
            Err(LocalCliError::InvalidCodexOutput)
        ));
    }

    #[test]
    fn only_selected_mcp_servers_are_enabled() {
        let installed = vec!["github".into(), "postgres".into(), "my.server".into()];
        let selected = vec!["postgres".to_owned()];
        let overrides = build_mcp_overrides(&installed, &selected).unwrap();
        assert_eq!(
            overrides,
            vec![
                "mcp_servers.\"github\".enabled=false",
                "mcp_servers.\"postgres\".enabled=true",
                "mcp_servers.\"my.server\".enabled=false",
            ]
        );
    }

    #[test]
    fn rejects_unknown_or_unsafe_mcp_servers() {
        let installed = vec!["github".into(), "bad\"name".into()];
        assert!(matches!(
            build_mcp_overrides(&installed, &["missing".into()]),
            Err(LocalCliError::McpNotFound(_))
        ));
        assert!(matches!(
            build_mcp_overrides(&installed, &["github".into()]),
            Err(LocalCliError::InvalidMcpName(_))
        ));
    }

    #[test]
    fn recognizes_subscription_without_exposing_tokens() {
        assert_eq!(
            classify_login_status(true, "Logged in using ChatGPT"),
            CodexAuth::ChatGptSubscription
        );
        assert_eq!(
            classify_login_status(true, "Logged in using an API key"),
            CodexAuth::ApiKey
        );
        assert_eq!(classify_login_status(false, ""), CodexAuth::NotSignedIn);
        assert_eq!(
            classify_login_status(true, "unknown mode"),
            CodexAuth::Unknown
        );
    }

    #[test]
    fn extracts_only_mcp_server_names() {
        let config = json!([
            {"name": "github", "command": "secret-password"},
            {"name": "postgres", "env": {"PASSWORD": "do-not-show"}},
            {"name": "github"}
        ]);
        let names = extract_mcp_names(&config);
        assert_eq!(names, vec!["github", "postgres"]);
        assert!(!format!("{names:?}").contains("do-not-show"));
        assert!(!format!("{names:?}").contains("secret-password"));
    }

    #[test]
    fn parses_object_form_config() {
        let config = json!({"mcp_servers": {"git": {}, "local": {}}});
        assert_eq!(extract_mcp_names(&config), vec!["git", "local"]);
    }
}
