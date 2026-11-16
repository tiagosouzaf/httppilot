# HttpPilot architecture

## Goals

HttpPilot aims to provide a native, cross-platform HTTP API client with predictable resource usage. All application logic is written in Rust; the desktop application uses Iced and does not embed Chromium or another WebView.

## Modules

- **httppilot-core:** HTTP request execution, reusable connection pool, timeout, bounded response preview. No dependency on GUI code.
- **httppilot-desktop:** Iced UI, state management, view logic, asynchronous task integration.
- **httppilot-cli:** Small executable using the same core; useful for testing independently of the GUI.

## Request flow

```text
Desktop or CLI
    -> HttpRequest (method, URL, headers, optional body)
    -> HttpEngine (Reqwest connection pool + timeout)
    -> HTTP response chunks
    -> bounded preview + status + headers + elapsed time
    -> UI / terminal
```

## Design principles

1. Keep the networking engine independent of the UI.
2. Avoid blocking the GUI thread on network operations.
3. Limit the amount of response data retained in memory.
4. Never persist tokens in plain text; integrate OS credential storage before adding saved secrets.
5. Benchmark across Windows and Linux with measured workloads, not speculative marketing numbers.
6. Permit operation without user accounts or cloud services.

## Known MVP limitations

- The current UI cannot edit arbitrary headers or manage collections.
- Body editing currently assumes JSON when nonempty and does not support multipart or binary uploads.
- Response previews are converted to lossy UTF-8; binary files require a dedicated streaming download path.
- Cancellation and persistent history are not implemented yet.
- Large response previews are bounded, but the current editor is not virtualized.
- No benchmarks or build validation have been run yet.

## Next milestones

1. Persist collections, tabs, history and environments in SQLite.
2. Add headers, query parameters, Basic and Bearer auth.
3. Implement explicit request cancellation and safe binary download to disk.
4. Integrate syntax highlighting and structured/virtualized JSON inspection.
5. Add import/export, automated tests, CI and distributable packages.
