mod access;
mod agent_process;
mod agents;
mod bridge;
mod classifier;
mod config;
mod config_cli;
mod connection_test;
mod hosted_search;
mod protocol;
mod random_port;
mod tui;
mod update;
mod web_search;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "wmlm",
    version,
    about = "Launch coding agents with your own LLM backend",
    subcommand_precedence_over_arg = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<ManagementCommand>,
    /// Use an alternate credential file
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,
    /// Override this agent's default backend
    #[arg(long)]
    backend: Option<String>,
    /// Override the configured model
    #[arg(long)]
    model: Option<String>,
    /// Print automatic arguments as a JSON array and exit (-arg also accepted)
    #[arg(long = "arg")]
    show_arguments: bool,
    /// Disable automatic arguments; keep backend environment configuration
    #[arg(short = 'x', long = "no-auto-args")]
    no_auto_args: bool,
    /// Installed coding agent: pi, opencode, codex, or claude
    agent: Option<String>,
    /// Arguments forwarded to the coding agent after --
    #[arg(last = true)]
    args: Vec<String>,
}

#[derive(clap::Subcommand)]
enum ManagementCommand {
    /// Configure backends and agents (opens the TUI without a subcommand)
    Cfg(Box<config_cli::Options>),
    /// Update wmlm from the latest stable GitHub release
    Update(update::Options),
}

// Clap short flags are single characters; support the requested -arg spelling
// only on wmlm's side of the separator, leaving forwarded arguments untouched.
fn normalize_arguments(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Vec<std::ffi::OsString> {
    let mut forwarded = false;
    let mut value = false;
    args.into_iter()
        .map(|arg| {
            if forwarded {
                return arg;
            }
            if value {
                value = false;
                return arg;
            }
            if arg == "--" || arg == "cfg" {
                forwarded = true;
            }
            if arg == "--config" || arg == "--backend" || arg == "--model" {
                value = true;
            }
            if arg == "-arg" {
                "--arg".into()
            } else {
                arg
            }
        })
        .collect()
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse_from(normalize_arguments(std::env::args_os()));
    if cli.command.is_some()
        && (cli.agent.is_some()
            || cli.backend.is_some()
            || cli.model.is_some()
            || cli.show_arguments
            || cli.no_auto_args
            || !cli.args.is_empty())
    {
        Cli::command()
            .error(
                clap::error::ErrorKind::ArgumentConflict,
                "cfg and update cannot be combined with agent launch arguments",
            )
            .exit();
    }
    match run(cli).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("wmlm: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<u8> {
    match cli.command {
        Some(ManagementCommand::Cfg(options)) => {
            config_cli::run(*options, cli.config.as_deref()).await?;
            return Ok(0);
        }
        Some(ManagementCommand::Update(options)) => {
            update::run(options).await.context("update failed")?;
            return Ok(0);
        }
        None => (),
    }
    let agent = cli
        .agent
        .context("specify an agent (pi, opencode, codex, claude), or run wmlm cfg")?;
    let agent = if agent == "claude-code" {
        "claude".to_owned()
    } else {
        agent
    };
    agents::protocol(&agent)?;
    let args = if agent == "opencode" && !cli.no_auto_args {
        let args = agents::opencode_args(&cli.args).await?;
        if args != cli.args && agents::opencode_argument_warning(&cli.args) {
            eprintln!("wmlm: warning: automatically added --standalone may be rejected or conflict with this OpenCode command; use wmlm opencode -x -- ... to disable automatic arguments, or -arg to inspect them");
        }
        args
    } else {
        cli.args.clone()
    };
    if cli.show_arguments && (agent == "opencode" || cli.no_auto_args) {
        let added: Vec<&str> = if args != cli.args {
            vec!["--standalone"]
        } else {
            vec![]
        };
        println!("{}", serde_json::to_string(&added)?);
        return Ok(0);
    }

    let config = match cli.config {
        Some(path) => config::Config::load_from(&path)?,
        None => config::Config::load()?,
    };
    let binding = config.agents.get(&agent);
    let backend_name = cli
        .backend
        .as_deref()
        .or_else(|| binding.map(|b| b.backend.as_str()))
        .context("no backend selected; run wmlm cfg or supply --backend NAME")?;
    let backend = config
        .backends
        .get(backend_name)
        .with_context(|| format!("backend {backend_name:?} does not exist; run wmlm cfg"))?
        .clone();
    let model = cli
        .model
        .as_deref()
        .or_else(|| {
            binding
                .filter(|b| b.backend == backend_name)
                .and_then(|b| b.model.as_deref())
        })
        .or(backend.model.as_deref())
        .filter(|m| !m.is_empty())
        .map(str::to_owned);
    if cli.show_arguments {
        let added = agents::automatic_arguments(
            &agent,
            "<bridge-url>/v1",
            Some(model.as_deref().unwrap_or("<discovered-model>")),
            backend.protocol,
            None,
        );
        println!("{}", serde_json::to_string(&added)?);
        return Ok(0);
    }
    // Install signal handlers before any access command is started.
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    // Assign immediately before starting backend access. Fixed configurations
    // need no local listening port, even if all candidates are occupied.
    let (backend, search) = if backend.uses_random_port() || config.web_search.uses_random_port() {
        let port = random_port::choose(&config.random_port_candidates)?;
        (backend.with_port(port), config.web_search.with_port(port))
    } else {
        (backend, config.web_search)
    };
    let mut access = access::AccessSession::new(backend.access.clone());
    let result = launch(
        &agent,
        &args,
        !cli.no_auto_args,
        backend,
        model,
        search,
        &mut access,
        (&mut terminate, &mut interrupt),
    )
    .await;
    access.cleanup().await;
    result
}

#[allow(clippy::too_many_arguments)]
async fn launch(
    agent: &str,
    args: &[String],
    automatic: bool,
    backend: config::Backend,
    model: Option<String>,
    search: config::SearchProvider,
    access: &mut access::AccessSession,
    signals: (
        &mut tokio::signal::unix::Signal,
        &mut tokio::signal::unix::Signal,
    ),
) -> Result<u8> {
    let (terminate, interrupt) = signals;
    tokio::select! {
        result = access.start(&backend.url) => result?,
        _ = terminate.recv() => return Ok(143),
        _ = interrupt.recv() => return Ok(130),
    }
    let upstream_protocol = backend.protocol;
    let bridge = bridge::Bridge::start_with_search(backend, model.clone(), search).await?;
    let discovered_models = if model.is_none() {
        tokio::select! {
            result = bridge.model_ids() => result?,
            never = access.supervise(&bridge) => match never {},
            _ = terminate.recv() => return Ok(143),
            _ = interrupt.recv() => return Ok(130),
        }
    } else {
        Vec::new()
    };
    let mut launch = agents::prepare_with_arguments(
        agent,
        &bridge.base_url,
        &bridge.token,
        model.as_deref(),
        args,
        upstream_protocol,
        &discovered_models,
        automatic,
    )?;
    let mut child =
        agent_process::AgentProcess::spawn(launch.take_command()).with_context(|| {
            format!("could not launch {agent}; install the agent and ensure it is on PATH")
        })?;
    let status = tokio::select! {
        status = child.wait() => status?,
        never = access.supervise(&bridge) => match never {},
        _ = terminate.recv() => {
            eprintln!("wmlm: received SIGTERM; stopping {agent}");
            child.shutdown(libc::SIGTERM).await;
            return Ok(143);
        }
        _ = interrupt.recv() => {
            eprintln!("wmlm: received SIGINT; stopping {agent}");
            child.shutdown(libc::SIGINT).await;
            return Ok(130);
        }
    };
    eprintln!("wmlm: {agent} exited with {status}");
    drop(child);
    // Keep temporary agent configuration and the proxy alive until the child exits.
    drop(launch);
    drop(bridge);
    use std::os::unix::process::ExitStatusExt;
    Ok(status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
        .clamp(0, 255) as u8)
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn automatic_argument_controls_respect_the_separator_and_option_values() {
        for args in [
            vec![
                "wmlm",
                "opencode",
                "-arg",
                "-x",
                "--",
                "-arg",
                "-x",
                "-c",
                "session-id",
            ],
            vec![
                "wmlm",
                "--arg",
                "--no-auto-args",
                "opencode",
                "--",
                "-arg",
                "-x",
                "-c",
                "session-id",
            ],
        ] {
            let cli =
                Cli::try_parse_from(normalize_arguments(args.into_iter().map(Into::into))).unwrap();
            assert!(cli.show_arguments);
            assert!(cli.no_auto_args);
            assert_eq!(cli.args, ["-arg", "-x", "-c", "session-id"]);
        }
        let args = normalize_arguments(
            ["wmlm", "--config", "-arg", "opencode", "-arg"]
                .into_iter()
                .map(Into::into),
        );
        assert_eq!(args[2], "-arg");
        assert_eq!(args[4], "--arg");
    }

    #[test]
    fn config_path_is_global_and_forwarded_agent_arguments_stay_literal() {
        for args in [
            vec!["wmlm", "--config", "settings.json", "cfg"],
            vec!["wmlm", "cfg", "--config", "settings.json"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(matches!(cli.command, Some(ManagementCommand::Cfg(_))));
            assert_eq!(
                cli.config.unwrap(),
                std::path::PathBuf::from("settings.json")
            );
            assert!(cli.agent.is_none());
        }
        let cli = Cli::try_parse_from(["wmlm", "claude", "--", "-cfg", "update", "cfg"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.agent.as_deref(), Some("claude"));
        assert_eq!(cli.args, ["-cfg", "update", "cfg"]);
    }
}
