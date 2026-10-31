//! Noninteractive configuration commands; persistence and validation are shared with the TUI.
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde_json::json;
use std::{io::Read, path::PathBuf};

use crate::config::{self, Access, AgentBinding, Auth, Backend, Config, Protocol, SearchProvider};

#[derive(Args)]
pub struct Options {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show configuration as JSON (credentials are redacted by default)
    Show {
        /// Include authentication credentials and search API keys
        #[arg(long)]
        show_secrets: bool,
    },
    /// Add, edit, delete, or test a backend
    Backend {
        #[command(subcommand)]
        command: BackendCommand,
    },
    /// Bind an agent to a backend (omitting --model uses the backend default)
    Bind {
        #[arg(value_parser = ["pi", "opencode", "codex", "claude"])]
        agent: String,
        backend: String,
        #[arg(long)]
        model: Option<String>,
    },
    /// Remove an agent binding
    Unbind {
        #[arg(value_parser = ["pi", "opencode", "codex", "claude"])]
        agent: String,
    },
    /// Select and configure the web search provider
    WebSearch {
        #[command(subcommand)]
        provider: SearchCommand,
    },
    /// Set random port candidates (individual ports and ranges)
    Ports { candidates: String },
}

#[derive(Subcommand)]
enum BackendCommand {
    /// Add a backend; --url and --protocol are required
    Add {
        name: String,
        #[command(flatten)]
        options: BackendOptions,
    },
    /// Edit a backend, preserving omitted values
    Edit {
        name: String,
        #[command(flatten)]
        options: BackendOptions,
    },
    /// Delete a backend and its agent bindings
    Delete { name: String },
    /// Test connectivity and list models; may run the saved access/cleanup commands
    Test { name: String },
}

#[derive(Args, Default)]
struct BackendOptions {
    /// Backend HTTP(S) base URL; {PORT} resolves to a session port
    #[arg(long)]
    url: Option<String>,
    #[arg(long, value_parser = ["chat_completions", "responses", "anthropic"])]
    protocol: Option<String>,
    /// Default model (omit on add to discover models)
    #[arg(long, conflicts_with = "clear_model")]
    model: Option<String>,
    /// Use model discovery instead of a backend default
    #[arg(long)]
    clear_model: bool,
    /// Read an Auth JSON object from a file, or - for stdin
    #[arg(
        long,
        conflicts_with = "clear_auth",
        long_help = r#"Read authentication JSON from a file, or - for stdin.
Bearer: {"type":"bearer","token":"YOUR_TOKEN"}
Basic: {"type":"basic","username":"YOUR_USERNAME","password":"YOUR_PASSWORD"}
Omit to preserve credentials on edit; --clear-auth removes them."#
    )]
    auth_file: Option<PathBuf>,
    /// Remove authentication
    #[arg(long)]
    clear_auth: bool,
    /// Shell command used to reach this backend
    #[arg(long, conflicts_with = "direct")]
    access_command: Option<String>,
    /// Remove the access command and connect directly
    #[arg(long, conflicts_with_all = ["persistent", "timeout_secs", "health_check_interval_secs", "cleanup", "clear_cleanup"])]
    direct: bool,
    /// Keep the access process alive (true/false); new commands default to false
    #[arg(long, action = clap::ArgAction::Set)]
    persistent: Option<bool>,
    /// Readiness timeout; new commands default to 30 seconds
    #[arg(long)]
    timeout_secs: Option<u64>,
    /// API health check interval; 0 disables, new commands default to 30 seconds
    #[arg(long)]
    health_check_interval_secs: Option<u64>,
    /// Shell command run when backend access ends
    #[arg(long, conflicts_with = "clear_cleanup")]
    cleanup: Option<String>,
    /// Remove the saved cleanup command
    #[arg(long)]
    clear_cleanup: bool,
}

#[derive(Subcommand)]
enum SearchCommand {
    /// DuckDuckGo public search (no API key)
    Public,
    /// SearXNG instance with JSON output enabled
    Searxng {
        #[arg(long)]
        url: String,
    },
    /// Brave Search; omit --api-key-file to keep an existing Brave key
    Brave {
        /// Plain text API key file, or - for stdin (one trailing line ending is removed)
        #[arg(long)]
        api_key_file: Option<PathBuf>,
    },
}

fn read_input(path: &std::path::Path) -> Result<String> {
    if path == std::path::Path::new("-") {
        let mut value = String::new();
        std::io::stdin()
            .read_to_string(&mut value)
            .context("Reading stdin")?;
        Ok(value)
    } else {
        std::fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))
    }
}

impl BackendOptions {
    fn apply(self, existing: Option<Backend>) -> Result<Backend> {
        let mut backend = existing.unwrap_or(Backend {
            url: String::new(),
            protocol: Protocol::ChatCompletions,
            model: None,
            auth: None,
            access: None,
        });
        if let Some(url) = self.url {
            backend.url = url;
        }
        if let Some(protocol) = self.protocol {
            backend.protocol = match protocol.as_str() {
                "chat_completions" => Protocol::ChatCompletions,
                "responses" => Protocol::Responses,
                "anthropic" => Protocol::Anthropic,
                _ => bail!("Unknown protocol"),
            };
        }
        if let Some(model) = self.model {
            backend.model = nonempty(model);
        }
        if self.clear_model {
            backend.model = None;
        }
        if let Some(path) = self.auth_file {
            // Serde errors may echo an invalid enum value supplied in a secret file.
            backend.auth = Some(serde_json::from_str::<Auth>(&read_input(&path)?)
                .map_err(|_| anyhow::anyhow!("Invalid auth JSON; expected bearer (type, token) or basic (type, username, password)"))?);
        }
        if self.clear_auth {
            backend.auth = None;
        }
        if self.direct {
            backend.access = None;
        }
        if let Some(command) = self.access_command {
            let access = backend.access.get_or_insert(Access {
                command: String::new(),
                persistent: false,
                cleanup: None,
                timeout_secs: 30,
                health_check_interval_secs: 30,
            });
            access.command = command;
        }
        let edits_access = self.persistent.is_some()
            || self.timeout_secs.is_some()
            || self.health_check_interval_secs.is_some()
            || self.cleanup.is_some()
            || self.clear_cleanup;
        if edits_access {
            let access = backend
                .access
                .as_mut()
                .context("Access options require an existing access command or --access-command")?;
            if let Some(value) = self.persistent {
                access.persistent = value;
            }
            if let Some(value) = self.timeout_secs {
                access.timeout_secs = value;
            }
            if let Some(value) = self.health_check_interval_secs {
                access.health_check_interval_secs = value;
            }
            if let Some(value) = self.cleanup {
                access.cleanup = nonempty(value);
            }
            if self.clear_cleanup {
                access.cleanup = None;
            }
        }
        Ok(backend)
    }
}

fn nonempty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn display_config(config: &Config, show_secrets: bool) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(config)?;
    if !show_secrets {
        for backend in value["backends"].as_object_mut().unwrap().values_mut() {
            if let Some(auth) = backend["auth"].as_object_mut() {
                for key in ["token", "username", "password"] {
                    if let Some(secret) = auth.get_mut(key) {
                        *secret = json!("[redacted]");
                    }
                }
            }
        }
        if let Some(secret) = value["web_search"].get_mut("api_key") {
            *secret = json!("[redacted]");
        }
    }
    Ok(value)
}

pub async fn run(options: Options, alternate_path: Option<&std::path::Path>) -> Result<()> {
    let Some(command) = options.command else {
        return crate::tui::run(alternate_path).await;
    };
    let path = match alternate_path {
        Some(path) => path.to_owned(),
        None => config::config_path()?,
    };
    let mut config = Config::load_from(&path)?;
    match command {
        Command::Show { show_secrets } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&display_config(&config, show_secrets)?)?
            );
            return Ok(());
        }
        Command::Backend { command } => match command {
            BackendCommand::Add { name, options } => {
                if config.backends.contains_key(&name) {
                    bail!("Backend {name:?} already exists; use cfg backend edit");
                }
                if options.url.is_none() || options.protocol.is_none() {
                    bail!("Adding a backend requires --url and --protocol");
                }
                config.backends.insert(name, options.apply(None)?);
            }
            BackendCommand::Edit { name, options } => {
                let existing = config
                    .backends
                    .get(&name)
                    .with_context(|| format!("Backend {name:?} does not exist"))?
                    .clone();
                config.backends.insert(name, options.apply(Some(existing))?);
            }
            BackendCommand::Delete { name } => {
                if config.backends.remove(&name).is_none() {
                    bail!("Backend {name:?} does not exist");
                }
                config.agents.retain(|_, binding| binding.backend != name);
            }
            BackendCommand::Test { name } => {
                let backend = config
                    .backends
                    .get(&name)
                    .with_context(|| format!("Backend {name:?} does not exist"))?;
                let models = crate::connection_test::test_connection(
                    backend,
                    &config.random_port_candidates,
                )
                .await?;
                println!(
                    "{}",
                    serde_json::to_string(&json!({"backend": name, "models": models}))?
                );
                return Ok(());
            }
        },
        Command::Bind {
            agent,
            backend,
            model,
        } => {
            config.agents.insert(
                agent,
                AgentBinding {
                    backend,
                    model: model.and_then(nonempty),
                },
            );
        }
        Command::Unbind { agent } => {
            if config.agents.remove(&agent).is_none() {
                bail!("Agent {agent:?} has no binding");
            }
        }
        Command::WebSearch { provider } => {
            config.web_search = match provider {
                SearchCommand::Public => SearchProvider::Public,
                SearchCommand::Searxng { url } => SearchProvider::Searxng { url },
                SearchCommand::Brave { api_key_file } => {
                    let api_key = match api_key_file {
                        Some(path) => {
                            let input = read_input(&path)?;
                            input
                                .strip_suffix("\r\n")
                                .or_else(|| input.strip_suffix('\n'))
                                .unwrap_or(&input)
                                .to_owned()
                        }
                        None => match &config.web_search {
                            SearchProvider::Brave { api_key } => api_key.clone(),
                            _ => bail!(
                                "Brave requires --api-key-file when no Brave key is configured"
                            ),
                        },
                    };
                    SearchProvider::Brave { api_key }
                }
            };
        }
        Command::Ports { candidates } => config.random_port_candidates = candidates,
    }
    config.save_at(&path)?;
    println!("{}", serde_json::to_string(&json!({"saved": path}))?);
    Ok(())
}
