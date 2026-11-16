//! Official local CLI integration. No token scraping or proxying of Claude
//! subscription credentials: Claude Code is launched as its own application.
//!
//! Codex subscription mode invokes the authenticated official CLI using a
//! read-only sandbox. Explicit opt-in enables the CLI's existing MCP config.
//! MCP tools may have remote side effects outside of the CLI filesystem sandbox.

use std::{io, process::Stdio, time::Duration};

use serde_json::Value;
use thiserror::Error;
use tokio::{io::AsyncWriteExt, process::Command};

const MAX_OUTPUT_BYTES: usize = 512 * 1024;
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
    if lower.contains("chatgpt") {
        CodexAuth::ChatGptSubscription
    } else if lower.contains("api key") || lower.contains("api_key") {
        CodexAuth::ApiKey
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
/// The prompt goes through stdin (never through process arguments), and only
/// the generated final answer is returned. The CLI inherits the user's own
/// MCP configuration ONLY after explicit opt-in via allow_mcp.
///
/// Note: configured MCP tools can access remote systems independently of the
/// read-only filesystem sandbox. Users must trust/approve those servers.
pub async fn ask_codex_subscription(
    prompt: &str,
    allow_mcp: bool,
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

    let mut command = Command::new("codex");
    command.args([
        "exec",
        "--ephemeral",
        "--sandbox",
        "read-only",
        "--skip-git-repo-check",
    ]);
    if !allow_mcp {
        // Codex's own switch prevents loading user MCP configuration.
        command.arg("--ignore-user-config");
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

    let mut child = command.spawn().map_err(map_codex_spawn)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).await?;
        stdin.flush().await?;
        drop(stdin);
    }

    let output =
        match tokio::time::timeout(Duration::from_secs(120), child.wait_with_output()).await {
            Ok(result) => result?,
            Err(_) => return Err(LocalCliError::CodexTimedOut),
        };
    if !output.status.success() {
        return Err(LocalCliError::CodexFailed);
    }
    if output.stdout.len() > MAX_OUTPUT_BYTES {
        return Err(LocalCliError::OutputTooLarge);
    }
    let content = String::from_utf8_lossy(&output.stdout).into_owned();
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
        return Ok(());
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
