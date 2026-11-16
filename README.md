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
```

On Linux, GUI packages for Wayland/X11 may be required depending on the distribution.

## Implemented in the prototype

- GET, POST, PUT, PATCH, DELETE and HEAD requests
- Native desktop with sidebar collections and multiple request tabs
- Request-name editor, editable headers, query parameters and request body
- Send button, HTTP response status, elapsed time and text preview
- Dedicated query parameter editor (Reqwest percent-encodes values, preserving existing URL query parameters)
- Switchable named environments and {{variable}} interpolation in URLs, query parameters, headers, body and authentication fields
- Bearer token and Basic authentication with **session-only** credentials
- Manual **Save** of workspace/collections/tabs/environments to local SQLite
- Reusable Reqwest HTTP connection pool and bounded 1 MiB response preview
- CI for Ubuntu and Windows

## Workspace security

Local data is stored in **unencrypted SQLite** in the OS-specific application data directory. HttpPilot removes common secret-bearing headers from saved snapshots. Environment variables marked **Secret** have their values excluded from SQLite; you must enter them again after restarting the app. Basic credentials and Bearer tokens entered in the **Authentication** section are kept in memory only and reset when switching requests.

**Important:** request bodies, URLs, query parameters, ordinary environment variables and custom headers are stored in plaintext. They may contain sensitive data. Do **not** place credentials there. Use `{{token}}` references to environment variables marked Secret in requests. Secrets are not yet persisted in a system keyring; protected storage is a planned milestone.

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

- OS keychain support for secure persistent credentials
- Automatic save with concurrency protection, collection renaming and export
- Postman/OpenAPI import/export and test scripts
- GraphQL, WebSocket, gRPC and binary file streaming
- Benchmarking, releases and Windows/Linux installers

A public GitHub repository is not automatically open-source licensed. Licensing has not yet been chosen.
