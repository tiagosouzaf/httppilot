# HttpPilot

High-performance, cross-platform API client built in Rust for **Windows** and **Linux**.

> Early development: this repository contains a functional HTTP request/response prototype, not a full Postman replacement.

## Goals

- Native desktop GUI powered by [Iced](https://iced.rs/), without Electron or a WebView
- Reusable, asynchronous HTTP engine built on Tokio and Reqwest
- Bounded response previews to avoid accidentally loading huge responses into the UI
- Local-first collections, environments, history and authentication (planned)
- A CLI built on the same engine
- Measure latency, startup time and memory usage before claiming performance improvements

## Getting started

Install the stable [Rust toolchain](https://rustup.rs/) (Rust 1.88+).

```bash
git clone https://github.com/tiagosouzaf/httppilot.git
cd httppilot
cargo run -p httppilot-desktop
```

A CLI is also available:

```bash
cargo run -p httppilot-cli -- GET https://httpbin.org/get
```

On Linux, the graphical stack may require your distribution's Wayland/X11 development libraries.

## Structure

```text
crates/
  httppilot-core/     # reusable HTTP execution engine
  httppilot-desktop/  # desktop UI in Iced
  httppilot-cli/      # minimal CLI for smoke tests and automation
docs/
  architecture.md     # technical decisions and roadmap
```

## Initial capabilities

- GET, POST, PUT, PATCH, DELETE and HEAD requests
- Editable URL and multiline request body
- HTTP status, elapsed time and response preview
- Request timeouts and reusable connection pooling via Reqwest
- Preview limited to 1 MiB by default; this is **not** a full streaming viewer yet

## Roadmap

1. Collections, tabs, request headers, local SQLite persistence
2. Environments, variable interpolation, auth (Bearer/Basic/OAuth2)
3. History, import/export (Postman/OpenAPI), request scripting
4. WebSocket, GraphQL and gRPC, plus streaming binary downloads
5. Performance benchmarks and Windows/Linux installers

## Development

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

No credentials, secrets, or personal API data should be committed. A public repository does not automatically confer an open-source license; licensing is still to be decided.
