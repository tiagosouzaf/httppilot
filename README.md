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
- Postman Collection v2.1 import/export via CLI (no Postman account or cloud required)
- CI for Ubuntu and Windows

## Workspace security

Local data is stored in **unencrypted SQLite** in the OS-specific application data directory. HttpPilot removes common secret-bearing headers from saved snapshots. Environment variables marked **Secret** have their values excluded from SQLite; you must enter them again after restarting the app. Authentication credentials are session-only unless explicitly saved using the keychain button. Use Load credentials to restore them for a request, and Delete credentials to remove them. On Linux, a running Secret Service keyring is required; on Windows, the Windows credential store is used. Keychain failures are shown in the application and never trigger a plaintext fallback.

**Important:** request bodies, URLs, query parameters, ordinary environment variables and custom headers are stored in plaintext. Imported Postman requests may include secrets inside raw URLs or bodies even when explicit authentication and recognized secret headers are omitted. Inspect imported collections and exported JSON before sharing. They may contain sensitive data. Do **not** place credentials there. Use `{{token}}` references to environment variables marked Secret in requests. Secret environment variables remain session-only.

Autosave waits briefly after edits and flushes on normal window close. If another instance has changed the workspace, saving stops instead of overwriting its changes; keep the window open and preserve your edits before restarting. A force quit or system failure may still lose changes that have not reached SQLite.

## Source layout

```text
crates/
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

- Native file-picker import/export UI, hierarchical Postman folders and OpenAPI import/export
- Test scripts and execution history
- GraphQL, WebSocket, gRPC and binary file streaming
- Benchmarking, releases and Windows/Linux installers

A public GitHub repository is not automatically open-source licensed. Licensing has not yet been chosen.
