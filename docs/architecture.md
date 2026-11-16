# HttpPilot architecture

## Goals

HttpPilot is a native, cross-platform API client for Windows and Linux. The entire desktop is built in Rust using Iced, without Electron, Chromium, or a WebView.

## Workspace modules

- **httppilot-core:** Reqwest/Tokio HTTP engine, pooled connections, timeout, template interpolation, bounded response preview.
- **httppilot-storage:** local SQLite workspace, optimistic revision checking, collections, environments, Postman v2.1 conversion and bounded request history metadata.
- **httppilot-desktop:** Iced UI, request tabs, editable headers/query params, collection renaming, authentication, autosave, cancellation, native file dialogs and history sidebar.
- **httppilot-cli:** HTTP requests and Postman Collection v2.1 import/export commands.

## Request flow

```text
Desktop or CLI
    -> single-pass environment expansion ({{name}})
    -> HttpRequest (method, URL, query, headers, auth, optional body)
    -> HttpEngine (Reqwest pool + timeout)
    -> HTTP response chunks
    -> bounded preview + status + headers + elapsed time
    -> UI / terminal
```

A response preview is capped at 1 MiB by default. Large responses require a future binary streaming/download implementation.

## Local state and concurrency

One JSON snapshot is stored in a SQLite row, with a separate revision row. Saving uses a compare-and-swap transaction: changes from another running instance cause a conflict rather than silent overwrites. The desktop debounces autosaves, saves through the blocking thread pool, and flushes on normal window close. UI changes that occur during an in-flight save remain dirty and are saved again.

A force quit can still lose unsaved edits. Workspaces are not yet normalized into individual SQLite tables.

## Credential handling

- Basic and Bearer credentials are session-only unless explicitly saved to the OS keychain.
- Common credential-bearing headers are excluded from persisted workspace snapshots.
- Environment variables tagged Secret are redacted when writing to SQLite.
- The Windows credential store and Linux Secret Service are used by the keychain integration. If a keychain call fails, there is no plaintext fallback.
- URLs, request bodies, ordinary environment variables, arbitrary headers and query parameters are still stored in plaintext and may contain sensitive data.

## Request history

SQLite stores the most recent 250 successful request executions in a separate table. Each record contains the saved request ID, HTTP method, status, duration and timestamp. The UI shows up to 30 records and can reopen an existing saved request. URLs, query parameters, headers, request/response bodies and authentication secrets are never written into history. Clearing history deletes these records, without changing the workspace revision.

## Postman interoperability

Postman Collection **v2.1** import/export is a pure storage-layer conversion exposed by the CLI and desktop; it never contacts Postman's servers. An import appends a new collection with new IDs, preserving existing requests and tabs. Folders are flattened into request-name prefixes (such as `Users / Get one`); no Postman script, authentication definition, form-data body, or nested folder model is imported. Unsupported body modes and methods are rejected rather than silently converted.

Explicit authentication metadata and common secret-bearing headers are ignored during import/export. A raw URL or body may still contain secrets: **review imported/exported JSON carefully**. Postman export creates a new file only; it refuses to overwrite an existing file. Desktop file selection uses PowerShell Windows Forms dialogs on Windows, Zenity or KDialog on Linux where installed, and falls back to a manual path field.

## Performance principles

1. HTTP engine and persistence are UI independent.
2. I/O occurs asynchronously, or off the GUI thread for SQLite and keychain operations.
3. Limit preview size and avoid accumulating response payloads unboundedly.
4. Share HTTP connection pools between requests.
5. Use benchmark measurements before claiming speed or memory improvements.
6. Do not require cloud accounts, telemetry or a remote database.

## Known limitations / roadmap

1. Provide native desktop portal file pickers on all Linux setups and fully hierarchical collection folders.
2. Add OpenAPI import/export, request scripts, test assertions and execution history.
3. Add streaming downloads, GraphQL, WebSocket and gRPC.
4. Introduce virtualization for large request lists and structured JSON viewers.
5. Build benchmarks, packaging and installers for Windows/Linux.
