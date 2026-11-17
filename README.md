# HttpPilot

High-performance native API client built in Rust for **Windows** and **Linux**. No Electron or WebView.

> Early-stage development: this is a prototype, not yet a full replacement for Postman.

## Stack

- Iced (desktop interface)
- Tokio + Reqwest (asynchronous HTTP)
- SQLite / Rusqlite (local-first workspace)
- Shared Rust HTTP engine and optional CLI

## Installers (Windows / Omarchy)

Preview installers are built automatically by [Native Installers GitHub Actions](https://github.com/tiagosouzaf/httppilot/actions/workflows/installers.yml) after each `main` update and published as [prereleases](https://github.com/tiagosouzaf/httppilot/releases). These are **unsigned developer previews**, not a security-audited stable release.

**Omarchy / Arch Linux, x86_64** (recommended for Arch-based desktop systems):

1. Open [GitHub Releases](https://github.com/tiagosouzaf/httppilot/releases) and download the newest `httppilot-0.1.0-1-x86_64.pkg.tar.zst`.
2. Open a terminal in the folder containing the downloaded file.
3. Install using the system package manager:

   ```bash
   sudo pacman -U ./httppilot-0.1.0-1-x86_64.pkg.tar.zst
   ```

4. Launch **HttpPilot** from the applications menu or run `httppilot`. The package also installs the `httppilot-cli` command. It relies on the system SQLite and D-Bus libraries. To remove it, run `sudo pacman -R httppilot`.

**Windows x64:** Download `HttpPilot-0.1.0-Windows-x64-Setup.exe` from [Releases](https://github.com/tiagosouzaf/httppilot/releases), run it, then launch **HttpPilot** using the Start Menu shortcut. The installer is per-user, requires no administrator privileges, installs the desktop and CLI binaries, and supports uninstallation through Windows Apps settings.

**Integrity:** Each prerelease includes a `SHA256SUMS.txt` file to verify downloaded installers. Since binaries are **not code-signed**, Windows SmartScreen and other security software may warn; inspect the GitHub source and release before installing. Never disable security tools globally to run a preview.

For maintainers, the installer definitions are in `packaging/arch/PKGBUILD` and `packaging/windows/HttpPilot.iss`. GitHub Actions compiles the Arch package *inside an Arch Linux container*, compiles the Windows binaries with the stable Rust toolchain, builds an Inno Setup installer, and tests a silent installation in the Windows CI runner. Building packages does **not** validate interactive GUI operation or users' local AI/MCP sign-ins. Installer sources are intentionally contained in the repository for review.

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
cargo run -p httppilot-cli -- import-openapi ./openapi-3.json
cargo run -p httppilot-cli -- export-postman 1 ./exported-collection.json
```

In the desktop sidebar, use **Open file** to choose a JSON file, then **Import Postman v2.1** or **Import OpenAPI 3.x**. For export, use **Save as** to select a *new* JSON filename and click **Export** beside the collection. Windows uses the system file dialog via PowerShell; Linux looks for Zenity or KDialog. If no dialog is available, type the full path manually. File operations run off the UI thread.

Postman and OpenAPI imports append a new local collection instead of overwriting existing data. Collection IDs can be listed with `list-collections`. Export refuses to overwrite an existing file, and on Unix newly exported files have user-only permissions. Only raw request bodies and common HTTP methods are currently supported. Nested Postman folders become request-name prefixes.

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
- Postman Collection v2.1 import/export and OpenAPI 3.0–3.2 JSON import from desktop and CLI (offline)
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

The **List local MCPs** action reads **only server names** from `codex mcp list --json` on your machine; it does not display or copy secret configuration. MCP use is **off by default**. After scanning, explicitly select each MCP server you trust and turn MCP on. Each Codex invocation re-reads local MCP server names and sets temporary enabled/disabled overrides for the selected servers without editing your config. Unknown or unusual server names cause a safe error instead of being enabled automatically. **MCP tools can modify remote systems**, even when Codex is operating with a read-only filesystem sandbox. Allow only trusted MCPs; remote tool approvals are handled by Codex and the server.

The assistant sends **only the prompt explicitly entered in its own editor**. It never automatically sends request URLs, API headers, HTTP payloads, responses, environment variables or workspace history.

The Codex CLI runs with `--sandbox read-only`, `--ephemeral`, and without inheriting `OPENAI_API_KEY` or `CODEX_API_KEY` from HttpPilot's environment. When no MCP servers are selected or MCP is off, it uses `--ignore-user-config`. It requests JSONL events with `--json`, extracts **only the final assistant message**, and limits raw event buffering to 4 MiB; tool outputs are never displayed verbatim. No browser session scraping, OAuth token extraction or unofficial subscription proxy is involved.

**Official documentation:** [Codex CLI](https://learn.chatgpt.com/docs/cli/reference), [Claude Code subscriptions](https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-max-or-ultra-plan), [Claude Code authentication rules](https://code.claude.com/docs/en/legal-and-compliance).

## OpenAPI JSON import

OpenAPI **3.0, 3.1 and 3.2** JSON documents can be imported entirely offline using the desktop or `import-openapi` CLI command. The importer creates a new collection with HTTP operations, names based on tags and summaries, request URLs based on the specification's declared server, and editable query/header parameters.

Path, query and header parameters are imported as `{{variable}}` placeholders rather than inserting example values or credentials. If the specification does not declare a server, requests use `{{baseUrl}}` and you must configure that environment variable yourself. Server variable defaults and request-body examples are deliberately not copied; JSON/text/XML request bodies start empty. Explicit security schemes and cookie values are ignored.

**Limitations:** this first release accepts JSON only, not YAML. It does not fetch external `$ref` definitions, support multipart/binary request bodies, or import scripts/webhooks. The importer rejects unsupported operations or unresolved references instead of silently omitting them; each import is atomic. Relative server URLs are rejected to prevent guessing a target host. Maximum input file size is 10 MiB and maximum operation count is 3,000. Review every imported URL before sending requests.

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
- Native portal-based file picker for all Linux desktops, hierarchical Postman folders, YAML OpenAPI support and OpenAPI export
- Test scripts and execution history
- GraphQL, WebSocket, gRPC and binary file streaming
- Benchmarking and signed, stable Windows/Linux installers

A public GitHub repository is not automatically open-source licensed. Licensing has not yet been chosen.
