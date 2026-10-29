# with-my-llm

Run your preferred coding agent against a local or remote model backend. with-my-llm configures and launches the agent, translates API requests, and can set up an SSH tunnel or proxy for the session.

![with-my-llm connects your coding agent to a local or remote model, with an optional managed SSH tunnel or proxy.](docs/overview.svg)

Configure your backends once, then choose how to work:

| What you want to do | Command | What with-my-llm handles |
| --- | --- | --- |
| Code with a local model | `wmlm opencode --backend local` | Connects the agent to your saved backend and model. |
| Try another agent on the same model | `wmlm codex --backend local` | Adapts supported API requests to the backend's protocol. |
| Use a model on a remote GPU | `wmlm opencode --backend home-gpu` | Starts the configured SSH tunnel or proxy, waits for connectivity, and cleans up on exit. |
| Try a different model for one session | `wmlm opencode --model unsloth/Qwen3.8-27B-GGUF` | Overrides the model for that run without changing your saved default. |

Supported coding agents: **Pi**, **OpenCode**, **Codex (experimental)**, and **Claude Code (experimental)**.

Codex and Claude Code integrations are still under development; some tools and workflows may fail.

`local` and `home-gpu` are example backend names you create in the configuration UI. Launch with-my-llm from the project directory you want to work in.

The command-line executable is named `wmlm` (with my LLM).

## Quick start

You need Linux or macOS, Rust 1.86 or newer, an installed coding agent, and a running model API server. Have your server's URL, model ID, and any required credentials ready. Ensure the agent and Cargo's bin directory (usually `~/.cargo/bin`) are on `PATH`.

Install from this repository, then open the configuration UI:

```sh
cargo install --locked --path .
wmlm cfg
```

Choose **Add backend**, give it a name such as `local`, and enter its connection details:

- **URL:** the API base URL, such as `http://127.0.0.1:8000/v1`.
- **Protocol:** OpenAI Chat Completions, OpenAI Responses, or Anthropic Messages, matching your backend.
- **Model:** the exact model ID advertised by your backend, including any namespace.
- **Authentication:** a bearer token, HTTP Basic credentials, or none, as required by your backend.

Choose **Direct** for **Backend access** if the server is already reachable. For a remote server that needs a tunnel, follow [SSH tunnels and proxies](#ssh-tunnels-and-proxies).

Choose **Bind agent**, select your agent and backend, then **Exit**. From your project directory, start the agent:

```sh
wmlm opencode
```

Replace `opencode` with `pi`, `codex`, or `claude` to launch another agent. Forward agent options after `--`:

```sh
wmlm claude -- --continue
```

## Updating

Check for a newer stable release or install it:

```sh
wmlm update --check
wmlm update
```

Updates come from [minitoolz/with-my-llm GitHub Releases](https://github.com/minitoolz/with-my-llm/releases). The updater compares semantic versions and never downgrades or installs prereleases. Linux uses static musl binaries; macOS uses Darwin binaries. Both x86_64 and ARM64 are supported. Selection follows the running binary's architecture, including Intel binaries running under Rosetta.

The updater downloads the matching archive and its `.sha256` file, verifies SHA-256, and stages the extracted binary beside the current executable before replacing it atomically. Download, checksum, or extraction failures leave the existing executable intact. Configuration and credentials are preserved. The installation directory must be writable; otherwise the command reports an error without attempting privilege escalation. Symlinked launchers update their resolved executable. Package-manager installations should be updated through their package manager.

`--check` only reads release metadata and verifies that matching assets are listed; it does not download binaries or change files. A missing release, missing platform asset/checksum, or GitHub rate limit is reported as an error. Updates run only when explicitly requested.

### Publishing releases

Set the version in `Cargo.toml` and update `Cargo.lock`, then push a matching stable tag such as `v0.0.2`. CI validates the tag, runs its checks, and builds all four platform archives. Linux builds are checked for ELF interpreters and shared library dependencies. After all builds succeed, CI uploads the archives and their checksums to a draft GitHub Release and publishes it as latest. An interrupted draft upload can be retried; an already published release is never overwritten.

The updater expects `wmlm-<target>.tar.gz` containing exactly one regular file named `wmlm`, plus `wmlm-<target>.tar.gz.sha256`. The checksum detects corrupted downloads; it is not an independent publisher signature.

## Configuration

Run `wmlm cfg` to edit backends, agent defaults, credentials, and web-search settings. After adding or editing a backend, you can test the connection before it is saved. The test starts any configured access command, requests the authenticated model list, displays available models, and cleans up the access session. It does not generate a completion. A failed test reports the error but still saves your settings so you can edit them later.

A model supplied with `--model` overrides the agent's saved model, which overrides the backend's default.

Random-port substitution is always enabled when a `{PORT}` placeholder is used. Set **Random port candidates** in the main configuration menu to change the default `1024-65535`. Candidates can be single ports, inclusive ranges, or comma-separated combinations such as `3000, 8000 - 8100, 9000`. Spaces and tabs around `-` are optional and may be repeated. Ports must be between `1` and `65535`; empty entries and reversed ranges are rejected. Duplicate and overlapping candidates are counted once.

Use `{PORT}` in a backend URL, API key, access command, or cleanup command to insert the selected port. Matching ignores letter case, so `{port}` and `{PoRT}` also work. For example, combine `http://127.0.0.1:{PORT}/v1` with `ssh -N -L {port}:localhost:8000 user@host`. HTTP Basic credentials and web-search URLs/API keys also support the placeholder. Fixed values continue to work without a placeholder.

Immediately before starting backend access for a launch or connection test, with-my-llm picks a local TCP port that it can open, bind, and listen on. It skips this check when the backend has no `{PORT}` placeholder (unless the launch needs one for its web-search URL or API key). That port stays the same through access-command restarts and cleanup. with-my-llm saves the templates, not the substituted values. Availability checks cover local IPv4 and IPv6 (where supported), releasing the sockets before starting the access command, so another process can still claim the port during startup. Remote port availability is not checked. If no candidate can be opened and listened on, with-my-llm stops before launching the tunnel or coding agent and prints `Unable to assign a port in the port range: ...`. Once the tunnel starts, it owns the selected port; with-my-llm waits for backend readiness before launching the coding agent. In the credential JSON, the setting is `"random_port_candidates": "1024-65535"`; existing configurations use this default automatically.

Leave both model fields blank to discover models from the backend's OpenAI-compatible `GET /v1/models` endpoint. All agents initially select the first model returned by the backend. This discovered default does not pin requests to that model. OpenCode and Pi also register the returned models in their session catalogs for model selection.

The bridge also exposes the discovered catalog to clients that request it. Discovery runs after backend access is established, uses the backend's credentials, and caches the list for the session. Restart with-my-llm to refresh it.

If discovery fails or returns no models, with-my-llm reports an error before launching the agent; set `--model MODEL` for servers without a model-list endpoint.

Using a local model through with-my-llm leaves your ordinary Codex model default unchanged. Your existing Codex sessions and skills remain accessible, but model and settings changes made inside a with-my-llm-launched Codex session are discarded on exit. To change your usual Codex defaults, launch Codex directly.

If upgrading from codeport, copy your existing `~/.config/codeport/credential.json` to `~/.config/wmlm/credential.json`, keeping its restricted file permissions, or use `wmlm --config ~/.config/codeport/credential.json` followed by your usual arguments.

Configuration and credentials are stored in `~/.config/wmlm/credential.json`. Credentials are plaintext, protected by restricted file permissions. To use a separate configuration:

```sh
wmlm cfg --config ./credential.json
wmlm --config ./credential.json opencode
```

## SSH tunnels and proxies

with-my-llm can start a connection command before launching your agent and stop it when the session ends. For a backend already reachable from your machine, choose **Direct** under **Backend access**.

For example, to reach a model server listening on port `8000` on an SSH host named `home-gpu`, edit the backend in `wmlm cfg`:

1. Set the backend URL to `http://127.0.0.1:18000/v1`.
2. Choose **Command** under **Backend access** and enter:

   ```sh
   ssh -N -o BatchMode=yes -o ExitOnForwardFailure=yes -L 127.0.0.1:18000:127.0.0.1:8000 home-gpu
   ```

3. Choose **Stay running during the agent session** for the command lifecycle.
4. Save the backend, then launch with `wmlm opencode --backend home-gpu`.

Replace `home-gpu` and the ports with your own connection details. SSH must be installed and able to authenticate without a password prompt. with-my-llm waits for the local port to become reachable before launching the agent. After launch, an unexpected tunnel exit triggers reconnection while the agent stays running.

You can configure a proxy or another access utility the same way. Install and authenticate it beforehand; access commands cannot prompt for input. Commands that only prepare a connection can use **Finish before launching the agent**, with an optional cleanup command to stop any background service they start.

After initial backend readiness, with-my-llm automatically restarts persistent access commands that exit, with a one-second delay between attempts, while keeping the coding agent running. The command and its child processes are stopped when the agent exits. Preparation commands are not restarted just because they finish. A tunnel started separately from with-my-llm is not managed or restarted; configure its command under **Backend access** to enable this recovery.

For backends with an access command, with-my-llm also checks the authenticated model-list API every 30 seconds. Checks bypass the cached catalog and require a successful HTTP response containing a nonempty, valid model list; each request times out after 15 seconds. A failed check runs the cleanup command, stops the old access process and its children, then relaunches the access command after a one-second delay. This applies to both command lifecycles and keeps the agent running during recovery. Failed restarts and process-monitoring errors are retried without terminating the agent, and polling resumes after readiness succeeds. Cleanup errors are reported but do not prevent restarting.

Set **API health check interval in seconds** in `wmlm cfg`, or set `health_check_interval_secs` in the backend's `access` configuration:

```json
"access": {
  "command": "ssh -N -L 8000:localhost:8000 server",
  "persistent": true,
  "timeout_secs": 30,
  "health_check_interval_secs": 30,
  "cleanup": null
}
```

Use `0` to disable API polling, including for backends that do not support the model-list endpoint. Persistent commands are still restarted when their process exits. Health checks do not generate completions or verify model inference.


## Web search for Claude Code

Claude's built-in `WebSearch` works with public DuckDuckGo search by default when using a Chat Completions or Responses backend. No search API key is needed. Native Anthropic backends keep their own search service.

To change providers, open `wmlm cfg` → **Configure web search**:

- **Public:** DuckDuckGo, with no setup or API key.
- **SearXNG:** enter your instance's base URL and enable JSON results in its `search.formats` setting.
- **Brave:** enter your Brave Search API key.

Search queries go to the selected provider directly, without using the model backend's SSH tunnel or proxy. Public search may encounter rate limits or bot challenges; with-my-llm reports failures without switching providers automatically.

with-my-llm enables Claude's built-in `WebFetch` across domains for the session. Explicit deny rules and managed policies still apply. Websites may reject requests, require login, or depend on JavaScript, so some pages cannot be fetched.

## Compatibility and limitations

Codex and Claude Code support is experimental. Text conversations and tool calls are supported across API protocols, but some agent features require a matching backend protocol:

- **Codex:** reasoning and hosted web search are disabled when translating to another protocol.
- **Claude Code:** extended thinking and explicit reasoning effort are disabled when translating to another protocol.
- **Images and other advanced features:** may be unavailable during protocol translation.

Claude auto mode is available with Chat Completions and Responses backends:

```sh
wmlm claude -- --permission-mode auto
```

with-my-llm uses your configured model to review proposed actions, so permission decisions depend on that model and may add latency. Native web searches and fetches are automatically allowed by with-my-llm's reviewer; other actions remain blocked if their review fails. This review differs from Anthropic's own classifier.

Codex defaults to a 32,768-token context window. If your backend supports a different size, set it explicitly:

```sh
wmlm codex -- -c model_context_window=65536
```

## Troubleshooting

- **Agent or `wmlm` not found:** check that the executable is installed and on `PATH`.
- **401 Unauthorized:** update the backend's credentials with `wmlm cfg`.
- **Model not found:** check the exact model ID, including its namespace, and confirm the server has loaded it.
- **Connection timeout:** check the backend URL, server availability, and any SSH or proxy command. A reachable port alone does not confirm valid credentials or a loaded model.
- **Agent stops after a connection loss:** managed access recovery reports `backend access restored` when ready. Agent exits report their exit code or terminating signal; SIGTERM and SIGINT received by with-my-llm are reported separately. The launcher exits when the agent exits; an in-flight model request can fail during recovery. With **Direct** access, restart separately launched tunnels yourself or configure a managed access command.
- **WebFetch blocked or rejected:** restart with-my-llm after updating it. Website errors such as HTTP 402 or 403 can still prevent fetching even when with-my-llm allows the request.
