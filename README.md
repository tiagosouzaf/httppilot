# HttpPilot

High-performance native API client built in Rust for **Windows** and **Linux**. No Electron or WebView.

> Early-stage development: this is a prototype, not yet a full replacement for Postman.

## Stack

- Iced (desktop interface)
- Tokio + Reqwest (asynchronous HTTP)
- SQLite / Rusqlite (local-first workspace)
- Shared Rust HTTP engine and optional CLI

## Run locally

Requires the stable Rust toolchain (Rust 1.88+).

```bash
git clone https://github.com/tiagosouzaf/httppilot.git
cd httppilot
cargo run -p httppilot-desktop
```

CLI:

```bash
cargo run -p httppilot-cli -- GET https://httpbin.org/get
cargo run -p httppilot-cli -- list-collections
cargo run -p httppilot-cli -- import-postman ./postman-collection-v2.1.json
cargo run -p httppilot-cli -- export-postman 1 ./exported-collection.json
```

In the desktop sidebar, use **Open file** to choose a Postman JSON, then **Import collection**. For export, use **Save as** to select a *new* JSON filename and click **Export** beside the collection. Windows uses the system file dialog via PowerShell; Linux looks for Zenity or KDialog. If no dialog is available, type the full path manually. File operations run off the UI thread.

Postman imports append a new local collection instead of overwriting existing data. Collection IDs can be listed with `list-collections`. Export refuses to overwrite an existing file, and on Unix newly exported files have user-only permissions. Only raw request bodies and common HTTP methods are currently supported. Nested Postman folders become request-name prefixes.

On Linux, GUI packages for Wayland/X11 may be required depending on the distribution.

## Implemented in the prototype

- GET, POST, PUT, PATCH, DELETE and HEAD requests
- Native desktop with editable collection names and multiple request tabs
- Request-name editor, editable headers, query parameters and request body
- Send and Cancel buttons, HTTP response status, elapsed time and text preview
- Dedicated query parameter editor (Reqwest percent-encodes values, preserving existing URL query parameters)
- Switchable named environments and {{variable}} interpolation in URLs, query parameters, headers, body and authentication fields
- Bearer token and Basic authentication, optionally saved explicitly to the OS keychain
- Debounced automatic saving to SQLite, with revision checks to prevent conflicting instances from overwriting each other
- Reusable Reqwest HTTP connection pool and bounded 1 MiB response preview
- Postman Collection v2.1 import/export from the desktop and CLI (no Postman account or cloud required)
- Privacy-preserving SQLite history of successful HTTP responses (status, method, elapsed time, request ID only)
- Optional OpenAI/Claude API AI assistant, ChatGPT subscription via official Codex CLI, local MCP discovery, and separate official Claude Code launch
- CI for Ubuntu and Windows

## AI integration (opt-in)

HttpPilot supports three AI modes in the native desktop assistant panel:

1. **ChatGPT subscription via the official Codex CLI:** Install the official Codex CLI, then run `codex login` and sign in with **ChatGPT**. The panel can check `codex login status`, send a prompt through `codex exec`, and reuse MCP servers configured in your local Codex. The app rejects Codex installations authenticated only with an **API key** in this subscription mode, to avoid unexpected pay-per-use charges. Codex usage is governed by your ChatGPT/Codex subscription limits, not unlimited API access.
2. **OpenAI API:** Enter a key from OpenAI Platform to call the Responses API. Default model: `gpt-6-luna`, editable.
3. **Claude API:** Enter a key from Claude Console to call the Messages API. Default model: `claude-haiku-5-5`, editable.

API keys are session-only unless you **explicitly** press **Save in keychain**. Loading and deleting keys uses your operating system's keychain. No API keys are saved in SQLite.

**Claude subscription:** Anthropic's published policy does **not** permit a third-party app to route requests through a Claude.ai subscription. Instead, HttpPilot provides **Open official Claude Code**, launching the unmodified CLI in a separate terminal. This keeps the subscription login and existing Claude Code MCP servers inside Anthropic's own app. Inside HttpPilot, use a Claude API key.

### Local MCP tools

The **List local MCPs** action reads **only server names** from `codex mcp list --json` on your machine; it does not display or copy secret configuration. MCP use is **off by default**, and activating it allows the official Codex CLI to load your existing configured MCP servers in a task. **MCPs can modify remote systems**, even when Codex is operating with a read-only filesystem sandbox. Enable MCP only for servers and actions you trust.

The assistant sends **only the prompt explicitly entered in its own editor**. It never automatically sends request URLs, API headers, HTTP payloads, responses, environment variables or workspace history.

The Codex CLI runs with `--sandbox read-only`, `--ephemeral`, and without inheriting `OPENAI_API_KEY` or `CODEX_API_KEY` from HttpPilot's environment. When MCP mode is disabled, it uses `--ignore-user-config`. No browser session scraping, OAuth token extraction or unofficial subscription proxy is involved.

**Official documentation:** [Codex CLI](https://learn.chatgpt.com/docs/cli/reference), [Claude Code subscriptions](https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-max-or-ultra-plan), [Claude Code authentication rules](https://code.claude.com/docs/en/legal-and-compliance).

## Request history

Successful HTTP responses are recorded locally with method, status code, elapsed time and a reference to the saved request. The history keeps the newest 250 records and displays up to 30 in the desktop sidebar. Click an entry to open its saved request. History deliberately does **not** store URLs, headers, query parameters, bodies, tokens or response payloads. A **Clear history** button deletes recorded entries. This lightweight history is not a complete request/response archive.

## Workspace security

Local data is stored in **unencrypted SQLite** in the OS-specific application data directory. HttpPilot removes common secret-bearing headers from saved snapshots. Environment variables marked **Secret** have their values excluded from SQLite; you must enter them again after restarting the app. Authentication credentials are session-only unless explicitly saved using the keychain button. Use Load credentials to restore them for a request, and Delete credentials to remove them. On Linux, a running Secret Service keyring is required; on Windows, the Windows credential store is used. Keychain failures are shown in the application and never trigger a plaintext fallback.

**Important:** request bodies, URLs, query parameters, ordinary environment variables and custom headers are stored in plaintext. Imported Postman requests may include secrets inside raw URLs or bodies even when explicit authentication and recognized secret headers are omitted. Inspect imported collections and exported JSON before sharing. They may contain sensitive data. Do **not** place credentials there. Use `{{token}}` references to environment variables marked Secret in requests. Secret environment variables remain session-only.

Autosave waits briefly after edits and flushes on normal window close. If another instance has changed the workspace, saving stops instead of overwriting its changes; keep the window open and preserve your edits before restarting. A force quit or system failure may still lose changes that have not reached SQLite.

## Source layout

```text
crates/
  httppilot-ai/       # OpenAI/Claude API and official local CLI integration
  httppilot-core/     # reusable HTTP engine
  httppilot-storage/  # collections, tabs and SQLite storage
  httppilot-desktop/  # Iced native GUI
  httppilot-cli/      # CLI for smoke testing
docs/
  architecture.md
```

## Development

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Roadmap

- Explicit per-MCP-tool approval UI and richer native MCP diagnostics
- Native portal-based file picker for all Linux desktops, hierarchical Postman folders and OpenAPI import/export
- Test scripts and execution history
- GraphQL, WebSocket, gRPC and binary file streaming
- Benchmarking, releases and Windows/Linux installers

A public GitHub repository is not automatically open-source licensed. Licensing has not yet been chosen.
