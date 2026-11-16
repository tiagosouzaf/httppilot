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
- Request-name editor, editable headers and request body
- Send button, HTTP response status, elapsed time and text preview
- Manual **Save** of workspace/collections/tabs to local SQLite
- Reusable Reqwest HTTP connection pool and bounded 1 MiB response preview
- CI for Ubuntu and Windows

## Workspace security

Local data is stored in **unencrypted SQLite** in the OS-specific application data directory. HttpPilot removes common secret-bearing headers from saved snapshots, but **request bodies, URLs and other custom headers are not encrypted**. Do not store secrets in those fields. Secure credential storage is planned.

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

- Secure authentication and environments
- Collection editing, query parameters and autosave
- Postman/OpenAPI import/export and test scripts
- GraphQL, WebSocket, gRPC and binary file streaming
- Benchmarking, releases and Windows/Linux installers

A public GitHub repository is not automatically open-source licensed. Licensing has not yet been chosen.
