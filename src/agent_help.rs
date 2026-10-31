//! Runtime guidance embedded in the executable so it matches the installed CLI.
pub const INSTRUCTIONS: &str = r#"Instructions for coding agents using wmlm

Purpose
  Configure and launch pi, opencode, codex, or claude against a user-selected
  model API. wmlm manages backend access, a local API bridge, and agent settings.
  These instructions describe this executable, not a different repository build.

Discover before changing settings
  command -v wmlm
  wmlm --version
  wmlm cfg show
  wmlm cfg backend add --help
  wmlm cfg backend edit --help
  Use the same executable throughout. A checkout and an installed binary can
  have different capabilities even when their version strings are identical.
  wmlm (no arguments) and wmlm --help show the same general help.
  wmlm cfg without a subcommand opens an interactive TUI; use cfg subcommands
  for unattended work. --config PATH selects a separate configuration file and
  is accepted at any subcommand level. A missing file starts with empty defaults.

Inspect and edit
  wmlm cfg show
  wmlm cfg backend add local --url http://127.0.0.1:8000/v1 --protocol chat_completions
  wmlm cfg backend edit local --model MODEL
  wmlm cfg bind opencode local
  wmlm cfg bind claude local --model MODEL
  wmlm cfg unbind claude
  wmlm cfg ports '3000,8000-8010'
  wmlm cfg backend delete local
  Names, addresses, and MODEL above are examples: inspect existing settings and
  use the user's intended values. Add fails if the name exists; edit fails if it
  does not. Edit preserves omitted values, including credentials and access.
  --clear-model removes the backend model; --clear-auth removes authentication;
  --direct removes backend access commands; --clear-cleanup removes cleanup.
  Bind without --model uses the backend default. Delete also removes all agent
  bindings to that backend. Writes are validated and saved with mode 0600.
  Avoid concurrent edits: writes are atomic but do not merge concurrent changes.

Credentials and output
  cfg show outputs JSON and redacts auth credentials and Brave API keys.
  Access/cleanup command strings remain visible; do not embed secrets in them.
  Do not copy [redacted] back as a credential. Use show --show-secrets only when
  actual secret values are needed; avoid exposing them in logs or responses.
  backend add/edit --auth-file PATH reads an authentication JSON object; use -
  for stdin. Bearer: {"type":"bearer","token":"TOKEN"}
  Basic: {"type":"basic","username":"USER","password":"PASSWORD"}
  Keep existing credentials by omitting --auth-file.
  Mutations print {"saved":"PATH"}; tests print {"backend":"NAME","models":[...]}.
  Diagnostics and connection-test subprocess output go to stderr. Errors return
  nonzero exit status. Inspection and edits do not launch agents or access commands.

Remote vLLM through SSH or Teleport
  vLLM normally uses protocol chat_completions and an API base ending in /v1.
  The following is a template for an already running vLLM on remote port 8888.
  Replace HOST with the user's server; verify the remote listen address and any
  required API key. Authenticate to SSH/Teleport before starting managed access.
  For Teleport, inspect tsh status and tsh ssh --help on the local installation.

  wmlm cfg backend add remote --url 'http://127.0.0.1:{PORT}/v1' --protocol chat_completions --access-command 'tsh ssh -N -L 127.0.0.1:{PORT}:127.0.0.1:8888 HOST' --persistent true --timeout-secs 60
  wmlm cfg bind opencode remote
  wmlm cfg backend test remote
  wmlm opencode --backend remote

  For ordinary SSH, replace tsh ssh with ssh. Use backend edit for an existing
  backend. Quote {PORT}: wmlm chooses a local port and substitutes the same value
  in the URL and access command. --persistent true keeps a tunnel alive while
  the agent runs; false waits for a preparation command to finish. Access options
  include --cleanup COMMAND, --timeout-secs N, --health-check-interval-secs N
  (0 disables health checks). New access commands default to false and 30 seconds
  for both timing settings. Check forwarding permissions when a tunnel fails.

Test and choose a model
  wmlm cfg backend test NAME
  This runs saved access/cleanup commands and authenticated model discovery;
  it does not generate a completion or change saved settings. Run it when a
  connection test is within the user's requested scope, not for inspection alone.
  With no configured model, launch discovers models from /v1/models and chooses
  the first one. Use an advertised model ID rather than guessing a filesystem path.
  wmlm cfg backend edit NAME --model MODEL
  wmlm opencode --backend NAME --model MODEL
  --model overrides the agent binding, then the backend model. A launch with an
  explicit model skips discovery; backend test still requires the models endpoint.

Agent arguments
  Put wmlm options before -- and forwarded agent arguments after it.
  wmlm opencode -arg -- run 'PROMPT'
  wmlm opencode -- run 'PROMPT'
  wmlm opencode -x -- serve
  -arg/--arg previews automatic arguments as JSON without launching the agent or
  contacting the backend (OpenCode help may be probed). Runtime-only values can
  appear as placeholders. -x/--no-auto-args disables generated arguments, keeps
  backend environment configuration, and preserves all explicit user arguments.
  OpenCode may need automatic --standalone so a shared server does not miss this
  launch's backend settings. Known incompatible commands produce a warning, not
  an automatic bypass. Use -x when appropriate; it can also remove required
  provider/model/extension settings for other agents. Check that agent's help
  rather than assuming its command-line flags are stable across versions.

Web search
  wmlm cfg web-search public
  wmlm cfg web-search searxng --url http://127.0.0.1:8080
  wmlm cfg web-search brave --api-key-file PATH
  Brave reads a plain-text key (or stdin with -); omit the file to retain an
  existing Brave key. See each command's --help for supported options.
"#;
