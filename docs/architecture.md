# HttpPilot architecture

## Goals

HttpPilot is a native, cross-platform HTTP API client with predictable resource usage. All application logic is written in Rust; the desktop uses Iced, not Chromium or a WebView.

## Modules

- **httppilot-core:** HTTP request execution, reusable connection pool, timeout, bounded response preview. Independent of the GUI.
- **httppilot-storage:** workspace models (collections, requests, tabs), SQLite persistence, OS-specific local data directory.
- **httppilot-desktop:** Iced UI, request tabs, editable headers, state management, async tasks.
- **httppilot-cli:** small executable using the same core for testing and automation.

## Request flow

```text
Desktop or CLI
    -> HttpRequest (method, URL, headers, optional body)
    -> HttpEngine (Reqwest connection pool + timeout)
    -> HTTP response chunks
    -> bounded preview + status + headers + elapsed time
    -> UI / terminal
```

## Persistence

The current workspace is serialized as JSON in one SQLite row. This provides atomic local snapshots and an easy first migration path. The desktop Save button persists collections, requests, headers, bodies, and opened tabs. It does not yet autosave.

SQLite is stored in the OS-specific local application data directory resolved via `directories::ProjectDirs`.

**Security limitations:** common secret-bearing headers (Authorization, Cookie, X-API-Key, tokens/secrets) are omitted from persisted snapshots. Other custom header names, request URLs, and bodies **may contain secrets and are stored in plaintext**. Do not put credentials in those fields if you intend to persist the workspace. Before adding persisted authentication, use OS keyring integration and a threat model.

## Performance principles

1. HTTP engine remains independent of UI and storage.
2. HTTP runs asynchronously and SQLite saves run in a blocking threadpool.
3. Response previews have a 1 MiB memory limit, not unlimited buffering.
4. Reuse Reqwest connection pools via a shared client.
5. Compare startup time, RAM use, large JSON inspection and request concurrency on Windows/Linux.
6. Work offline without a required account or telemetry.

## Known limitations

- Response rendering is a simple text view, not virtualized.
- Request metadata and content preview are currently not optimized for large collections.
- Only manual saves are implemented; UI state updates are not persisted until Save.
- No environment variables, secure stored authentication, request cancellation or streaming binary download yet.
- Editing a request while a network call is pending does not cancel the in-flight request.
- A request body is sent only for non-GET/HEAD methods.
- The current workspace format has no migration/versioning yet.

## Next milestones

1. SQLite schema migrations and automatic/delayed save with concurrency protection.
2. Dedicated query parameter editor, Basic/Bearer auth via secure keyring, environments.
3. Explicit request cancellation and streaming downloads to disk.
4. Syntax highlighting, virtualized JSON inspection and large-response benchmarks.
5. Postman/OpenAPI import/export, test scripts and packaging.
