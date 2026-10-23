<!-- markdownlint-disable MD013 -->

# Trace Telemetry

When `LLMENV_TRACE_TIMING` is set, llmenv emits structured telemetry lines to
stderr during the export and hook phases. When `LLMENV_TRACE_FILE` is set, the
same lines are appended as JSON objects to the named file using `O_APPEND`, so
concurrent hooks and detached jobs never interleave output.

Two environment variables control where lines go:

- `LLMENV_TRACE_TIMING` — when set, every trace line is also written to stderr.
  This is the original path and remains the default for interactive shells.
- `LLMENV_TRACE_FILE` — when set, every trace line is appended as one JSON
  object to the named file using `O_APPEND`. Concurrent hooks and detached jobs
  use this path so their output reaches a file instead of a detached stderr.

The JSON marker (`llmenv-trace <json>`) carries a `"v": 1` field so a consumer
can distinguish formats when new fields are added. All timing values use
microseconds (`_us`).

## Line formats

### `[LLMENV_CACHE]`

Emitted when the content hash cache is checked.

```text
[LLMENV_CACHE] <name> <hit|miss> <duration>ms[ <extra>]
```

Fields:

- `<name>` — cache key (e.g. `content_hash`, `merge_sig`, `read_once`).
- `<hit|miss>` — whether the cache returned a valid entry.
- `<duration>ms` — elapsed time in milliseconds with three decimal places.
- `<extra>` — optional extra data (present only on misses, contains the lookup
  key).

### `[LLMENV_MATERIALIZE]`

Emitted after each adapter's materialize step when `LLMENV_TRACE_TIMING` is set.

```text
[LLMENV_MATERIALIZE] <adapter> written=<n> unchanged=<n> removed=<n> bytes=<n> <us>us
```

Fields:

- `<adapter>` — adapter name (e.g. `claude-code`, `crush`).
- `written=<n>` — number of files written by this adapter.
- `unchanged=<n>` — files that existed with the same hash.
- `removed=<n>` — files present in the cache folder but not in the manifest.
- `bytes=<n>` — total bytes of written files.
- `<us>us` — elapsed time in microseconds.

Metadata files (`llmenv-status.json`, `.llmenv-manifest.json`) are excluded from
all counts.

### `[LLMENV_SPAWN]`

Emitted when a spawned tool (MCP probe, drift check) completes.

```text
[LLMENV_SPAWN] <label> <duration>us exit=<code|timeout|spawn-error>
```

Fields:

- `<label>` — fixed string such as `probe:ssid` or `check-stale`. Never prints
  arguments, because they can carry secrets such as keychain item names.
- `<duration>us` — elapsed time in microseconds.
- `exit=<code|timeout|spawn-error>` — exit code, `timeout`, or `spawn-error`.

### `[LLMENV_MCP_CALL]`

Emitted for MCP handshake and tool calls.

Handshake:

```text
[LLMENV_MCP_CALL] <method> <duration>us[ status=<code>]
```

- `<method>` — `initialize` or `notifications/initialized`.
- `<duration>us` — elapsed time in microseconds.
- `status=<code>` — present only on failure, the HTTP status code.

Tool calls:

```text
[LLMENV_MCP_CALL] <tool> <duration>us
```

- `<tool>` — tool name (e.g. `icm_memory_recall`).
- `<duration>us` — elapsed time in microseconds.

Emitted on success only. Failure is logged via `tracing` instead, matching the
precedent of `emit_trace_timing` and `emit_cache_trace`.

### `llmenv-trace` JSON marker

Emitted at the end of the full export pipeline.

```json
llmenv-trace {"v": 1, "op": "hook_run", "ts": <epoch-seconds>, "pid": <pid>, "data": { ... }}
```

The `"data"` object carries per-phase timing fields. Each field is optional — a
phase that did not run is left out.

Fields:

- `config_load_us` — time to load configuration (always present).
- `scope_eval_us` — time to evaluate active scopes (present when scopes exist).
- `prep_us` — time to prepare the manifest (present when scopes exist).
- `mcp_us` — time for MCP calls (present when prep completes).

### `llmenv-trace` export marker

Emitted by `llmenv export` with per-phase timing.

```json
llmenv-trace {"v": 1, "op": "export", "ts": <epoch-seconds>, "pid": <pid>, "data": { ... }}
```

Fields:

- `config_load_us` — time to load configuration (always present).
- `detect_env_us` — time to detect the environment (present when env is set).
- `evaluate_active_us` — time to evaluate active scopes (present when scopes
  exist).
- `build_manifest_us` — time to build the merged manifest (present when scopes
  exist).
- `materialize_adapters_us` — time for adapter materialize (present when
  manifest exists).
- `render_vars_us` — time to render environment variables (present when
  adapters materialize).

## JSON trace file format

When `LLMENV_TRACE_FILE` is set, every trace line is appended as one JSON object
to the named file. The object carries `"ts"` (Unix-epoch-seconds), `"pid"`, and
`"line"` (the original text). A consumer can reconstruct the original text while
still being able to parse the file as JSON Lines.

```json
{"ts": "1729700000", "pid": 12345, "line": "[LLMENV_CACHE] content_hash hit 1.234ms"}
```

## Versioning

The `"v": 1` field in JSON markers allows consumers to distinguish formats when
new fields are added by later sub-issues. All timing values use microseconds
(`_us`). New trace line formats should use a distinct prefix (e.g.
`[LLMENV_<NAME>]`) so consumers can parse them without ambiguity.
