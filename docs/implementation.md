# Implementation notes

These details are for contributors maintaining with-my-llm. For setup and everyday usage, see the [README](../README.md).

## Updater behavior

Updates come from [minitoolz/with-my-llm GitHub Releases](https://github.com/minitoolz/with-my-llm/releases). The updater compares semantic versions and never downgrades or installs prereleases. Linux uses static musl binaries; macOS uses Darwin binaries. Both x86_64 and ARM64 are supported. Selection follows the running binary's architecture, including Intel binaries running under Rosetta.

The updater downloads the matching archive and its `.sha256` file, verifies SHA-256, and stages the extracted binary beside the current executable before replacing it atomically. Download, checksum, or extraction failures leave the existing executable intact. Configuration and credentials are preserved. The installation directory must be writable; otherwise the command reports an error without attempting privilege escalation. Symlinked launchers update their resolved executable. Package-manager installations should be updated through their package manager.

`--check` only reads release metadata and verifies that matching assets are listed; it does not download binaries or change files. A missing release, missing platform asset/checksum, or GitHub rate limit is reported as an error. Updates run only when explicitly requested.

## Random-port substitution

Random-port substitution is always enabled when a `{PORT}` placeholder is used. Set **Random port candidates** in the main configuration menu to change the default `1024-65535`. Candidates can be single ports, inclusive ranges, or comma-separated combinations such as `3000, 8000 - 8100, 9000`. Spaces and tabs around `-` are optional and may be repeated. Ports must be between `1` and `65535`; empty entries and reversed ranges are rejected. Duplicate and overlapping candidates are counted once.

Use `{PORT}` in a backend URL, API key, access command, or cleanup command to insert the selected port. Matching ignores letter case, so `{port}` and `{PoRT}` also work. For example, combine `http://127.0.0.1:{PORT}/v1` with `ssh -N -L {port}:localhost:8000 user@host`. HTTP Basic credentials and web-search URLs/API keys also support the placeholder. Fixed values continue to work without a placeholder.

Immediately before starting backend access for a launch or connection test, with-my-llm picks a local TCP port that it can open, bind, and listen on. It skips this check when the backend has no `{PORT}` placeholder (unless the launch needs one for its web-search URL or API key). That port stays the same through access-command restarts and cleanup. with-my-llm saves the templates, not the substituted values. Availability checks cover local IPv4 and IPv6 (where supported), releasing the sockets before starting the access command, so another process can still claim the port during startup. Remote port availability is not checked. If no candidate can be opened and listened on, with-my-llm stops before launching the tunnel or coding agent and prints `Unable to assign a port in the port range: ...`. Once the tunnel starts, it owns the selected port; with-my-llm waits for backend readiness before launching the coding agent. In the credential JSON, the setting is `"random_port_candidates": "1024-65535"`; existing configurations use this default automatically.

## Model discovery

Leave both model fields blank to discover models from the backend's OpenAI-compatible `GET /v1/models` endpoint. All agents initially select the first model returned by the backend. This discovered default does not pin requests to that model. OpenCode and Pi also register the returned models in their session catalogs for model selection.

The bridge also exposes the discovered catalog to clients that request it. Discovery runs after backend access is established, uses the backend's credentials, and caches the list for the session. Restart with-my-llm to refresh it.

If discovery fails or returns no models, with-my-llm reports an error before launching the agent; set `--model MODEL` for servers without a model-list endpoint.

## Managed access recovery

After initial backend readiness, with-my-llm automatically restarts persistent access commands that exit, with a one-second delay between attempts, while keeping the coding agent running. The command and its child processes are stopped when the agent exits. Preparation commands are not restarted just because they finish. A tunnel started separately from with-my-llm is not managed or restarted; configure its command under **Backend access** to enable this recovery.

For backends with an access command, with-my-llm also checks the authenticated model-list API every 30 seconds. Checks bypass the cached catalog and require a successful HTTP response containing a nonempty, valid model list; each request times out after 15 seconds. A failed check runs the cleanup command, stops the old access process and its children, then relaunches the access command after a one-second delay. This applies to both command lifecycles and keeps the agent running during recovery. Failed restarts and process-monitoring errors are retried without terminating the agent, and polling resumes after readiness succeeds. Cleanup errors are reported but do not prevent restarting.

## Automatic agent arguments

`-arg` is normalized to `--arg` before Clap parsing, only before the forwarding separator and outside values of wmlm's `--config`, `--backend`, and `--model` options. Preview prints JSON and returns before access commands, port assignment, model discovery, bridge startup, or agent launch. OpenCode preview runs only `opencode --help`; disabled automatic arguments skip that probe too. Other previews share the launch argument builder, using placeholders for runtime-only values and the selected backend protocol/model from configuration.

`-x`/`--no-auto-args` suppresses all generated CLI arguments, not environment configuration or temporary Codex home isolation. User arguments remain intact. With automatic arguments enabled, OpenCode's top-level help determines whether to insert `--standalone`, after the supplied arguments but before their first literal `--`, unless already present before that separator. Preview reports only newly added arguments.

Warnings are advisory and emitted on stderr only when wmlm actually adds `--standalone`. Known command hints match leading `serve`, `service`, `plugin add/remove`, and `mcp add`; explicit `--server`/`--server=...` before the literal separator also triggers a warning. This deliberately does not reproduce OpenCode's full, version-dependent parser: options before a subcommand may prevent a hint, and ambiguous option values may resemble flags. The bypass remains available regardless of detection. Known conflicts are documented in [OpenCode PR #58505](https://github.com/anomalyco/opencode/pull/58505).
