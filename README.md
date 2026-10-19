# codeport

Run your preferred coding agent against a local or remote model backend. Codeport configures and launches the agent, translates API requests, and can set up an SSH tunnel or proxy for the session.

![Codeport connects your coding agent to a local or remote model, with an optional managed SSH tunnel or proxy.](docs/overview.svg)

Configure your backends once, then choose how to work:

| What you want to do | Command | What Codeport handles |
| --- | --- | --- |
| Code with a local model | `codeport opencode --backend local` | Connects the agent to your saved backend and model. |
| Try another agent on the same model | `codeport codex --backend local` | Adapts supported API requests to the backend's protocol. |
| Use a model on a remote GPU | `codeport opencode --backend home-gpu` | Starts the configured SSH tunnel or proxy, waits for connectivity, and cleans up on exit. |
| Try a different model for one session | `codeport opencode --model unsloth/Qwen3.8-27B-GGUF` | Overrides the model for that run without changing your saved default. |

Supported coding agents: **Pi**, **OpenCode**, **Codex (experimental)**, and **Claude Code (experimental)**.

Codex and Claude Code integrations are still under development; some tools and workflows may fail.

`local` and `home-gpu` are example backend names you create in the configuration UI. Launch Codeport from the project directory you want to work in.

## Quick start

You need Linux or macOS, Rust 1.86 or newer, an installed coding agent, and a running model API server. Have your server's URL, model ID, and any required credentials ready. Ensure the agent and Cargo's bin directory (usually `~/.cargo/bin`) are on `PATH`.

Install from this repository, then open the configuration UI:

```sh
cargo install --locked --path .
codeport -cfg
```

Choose **Add backend**, give it a name such as `local`, and enter its connection details:

- **URL:** the API base URL, such as `http://127.0.0.1:8000/v1`.
- **Protocol:** OpenAI Chat Completions, OpenAI Responses, or Anthropic Messages, matching your backend.
- **Model:** the exact model ID advertised by your backend, including any namespace.
- **Authentication:** a bearer token, HTTP Basic credentials, or none, as required by your backend.

Choose **Direct** for **Backend access** if the server is already reachable. For a remote server that needs a tunnel, follow [SSH tunnels and proxies](#ssh-tunnels-and-proxies).

Choose **Bind agent**, select your agent and backend, then **Exit**. From your project directory, start the agent:

```sh
codeport opencode
```

Replace `opencode` with `pi`, `codex`, or `claude` to launch another agent. Forward agent options after `--`:

```sh
codeport claude -- --continue
```

## Configuration

Run `codeport -cfg` to edit backends, agent defaults, credentials, and web-search settings. After adding or editing a backend, you can test the connection before it is saved. The test starts any configured access command, requests the authenticated model list, displays available models, and cleans up the access session. It does not generate a completion. A failed test reports the error but still saves your settings so you can edit them later.

A model supplied with `--model` overrides the agent's saved model, which overrides the backend's default.

Random-port substitution is always enabled when a `{PORT}` placeholder is used. Set **Random port candidates** in the main configuration menu to change the default `1024-65535`. Candidates can be single ports, inclusive ranges, or comma-separated combinations such as `3000, 8000 - 8100, 9000`. Spaces and tabs around `-` are optional and may be repeated. Ports must be between `1` and `65535`; empty entries and reversed ranges are rejected. Duplicate and overlapping candidates are counted once.

Use `{PORT}` in a backend URL, API key, access command, or cleanup command to insert the selected port. Matching ignores letter case, so `{port}` and `{PoRT}` also work. For example, combine `http://127.0.0.1:{PORT}/v1` with `ssh -N -L {port}:localhost:8000 user@host`. HTTP Basic credentials and web-search URLs/API keys also support the placeholder. Fixed values continue to work without a placeholder.

Immediately before starting backend access for a launch or connection test, Codeport picks a local TCP port that it can open, bind, and listen on. It skips this check when the backend has no `{PORT}` placeholder (unless the launch needs one for its web-search URL or API key). That port stays the same through access-command restarts and cleanup. Codeport saves the templates, not the substituted values. Availability checks cover local IPv4 and IPv6 (where supported), releasing the sockets before starting the access command, so another process can still claim the port during startup. Remote port availability is not checked. If no candidate can be opened and listened on, Codeport stops before launching the tunnel or coding agent and prints `Unable to assign a port in the port range: ...`. Once the tunnel starts, it owns the selected port; Codeport waits for backend readiness before launching the coding agent. In the credential JSON, the setting is `"random_port_candidates": "1024-65535"`; existing configurations use this default automatically.

Leave both model fields blank to discover models from the backend's OpenAI-compatible `GET /v1/models` endpoint. All agents initially select the first model returned by the backend. This discovered default does not pin requests to that model. OpenCode and Pi also register the returned models in their session catalogs for model selection.

The bridge also exposes the discovered catalog to clients that request it. Discovery runs after backend access is established, uses the backend's credentials, and caches the list for the session. Restart Codeport to refresh it.

If discovery fails or returns no models, Codeport reports an error before launching the agent; set `--model MODEL` for servers without a model-list endpoint.

Using a local model through Codeport leaves your ordinary Codex model default unchanged. Your existing Codex sessions and skills remain accessible, but model and settings changes made inside a Codeport-launched Codex session are discarded on exit. To change your usual Codex defaults, launch Codex directly.

Configuration and credentials are stored in `~/.config/codeport/credential.json`. Credentials are plaintext, protected by restricted file permissions. To use a separate configuration:

```sh
codeport --config ./credential.json -cfg
codeport --config ./credential.json opencode
```

## SSH tunnels and proxies

Codeport can start a connection command before launching your agent and stop it when the session ends. For a backend already reachable from your machine, choose **Direct** under **Backend access**.

For example, to reach a model server listening on port `8000` on an SSH host named `home-gpu`, edit the backend in `codeport -cfg`:

1. Set the backend URL to `http://127.0.0.1:18000/v1`.
2. Choose **Command** under **Backend access** and enter:

   ```sh
   ssh -N -o BatchMode=yes -o ExitOnForwardFailure=yes -L 127.0.0.1:18000:127.0.0.1:8000 home-gpu
   ```

3. Choose **Stay running during the agent session** for the command lifecycle.
4. Save the backend, then launch with `codeport opencode --backend home-gpu`.

Replace `home-gpu` and the ports with your own connection details. SSH must be installed and able to authenticate without a password prompt. Codeport waits for the local port to become reachable before launching the agent. If the tunnel exits unexpectedly, the agent session stops too.

You can configure a proxy or another access utility the same way. Install and authenticate it beforehand; access commands cannot prompt for input. Commands that only prepare a connection can use **Finish before launching the agent**, with an optional cleanup command to stop any background service they start.

After initial backend readiness, Codeport automatically restarts persistent access commands that exit, with a one-second delay between attempts, while keeping the coding agent running. The command and its child processes are stopped when the agent exits. Preparation commands are not restarted just because they finish.

For backends with an access command, Codeport also checks the authenticated model-list API every 30 seconds. Checks bypass the cached catalog and require a successful HTTP response containing a nonempty, valid model list; each request times out after 15 seconds. A failed check runs the cleanup command, stops the old access process and its children, then relaunches the access command after a one-second delay. This applies to both command lifecycles and keeps the agent running during recovery. Failed restarts are retried, and polling resumes after readiness succeeds. Cleanup errors are reported but do not prevent restarting.

Set **API health check interval in seconds** in `codeport -cfg`, or set `health_check_interval_secs` in the backend's `access` configuration:

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

To change providers, open `codeport -cfg` → **Configure web search**:

- **Public:** DuckDuckGo, with no setup or API key.
- **SearXNG:** enter your instance's base URL and enable JSON results in its `search.formats` setting.
- **Brave:** enter your Brave Search API key.

Search queries go to the selected provider directly, without using the model backend's SSH tunnel or proxy. Public search may encounter rate limits or bot challenges; Codeport reports failures without switching providers automatically.

Codeport enables Claude's built-in `WebFetch` across domains for the session. Explicit deny rules and managed policies still apply. Websites may reject requests, require login, or depend on JavaScript, so some pages cannot be fetched.

## Compatibility and limitations

Codex and Claude Code support is experimental. Text conversations and tool calls are supported across API protocols, but some agent features require a matching backend protocol:

- **Codex:** reasoning and hosted web search are disabled when translating to another protocol.
- **Claude Code:** extended thinking and explicit reasoning effort are disabled when translating to another protocol.
- **Images and other advanced features:** may be unavailable during protocol translation.

Claude auto mode is available with Chat Completions and Responses backends:

```sh
codeport claude -- --permission-mode auto
```

Codeport uses your configured model to review proposed actions, so permission decisions depend on that model and may add latency. Native web searches and fetches are automatically allowed by Codeport's reviewer; other actions remain blocked if their review fails. This review differs from Anthropic's own classifier.

Codex defaults to a 32,768-token context window. If your backend supports a different size, set it explicitly:

```sh
codeport codex -- -c model_context_window=65536
```

## Troubleshooting

- **Agent or `codeport` not found:** check that the executable is installed and on `PATH`.
- **401 Unauthorized:** update the backend's credentials with `codeport -cfg`.
- **Model not found:** check the exact model ID, including its namespace, and confirm the server has loaded it.
- **Connection timeout:** check the backend URL, server availability, and any SSH or proxy command. A reachable port alone does not confirm valid credentials or a loaded model.
- **WebFetch blocked or rejected:** restart Codeport after updating it. Website errors such as HTTP 402 or 403 can still prevent fetching even when Codeport allows the request.
