mod config;
mod http;
mod player;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::{Context as _, Result};
use clap::{error::ErrorKind, Parser, Subcommand};
use cmrt_core::{check_workspace_update, run_workspace_update, RenderOptions};
use config::{
    core_config_from_runtime, validate_realtime_play_server_config, RealtimeServerConfig,
};
use http::run_realtime_play_server;
use player::{PlayerHandle, RealtimePlayer};

const RENDER_PREROLL_MS: u64 = 100;
const BUILD_COMMIT_HASH: &str = env!("BUILD_COMMIT_HASH");

#[derive(Debug, PartialEq, Eq)]
enum CliAction {
    Run,
    Update,
    Check,
    PrintHelp(String),
}

#[derive(Debug, Parser)]
#[command(
    name = "clap-mml-realtime-play-server",
    about = "Play Standard MIDI Files through a CLAP plugin",
    disable_help_subcommand = true,
    disable_version_flag = true,
    args_conflicts_with_subcommands = true,
    after_help = "CONFIG:\n    config_local_dir()/clap-mml-render-tui/config.toml\n\nHTTP:\n    GET /health\n    POST /play   request: Standard MIDI File bytes, Content-Type: audio/midi | audio/x-midi | application/octet-stream\n    POST /stop"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Stop running workspace binaries and reinstall them
    Update,
    /// Compare the embedded commit hash with the remote main branch
    Check,
}

fn parse_cli<I, T>(args: I) -> Result<CliAction>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    match Cli::try_parse_from(args) {
        Ok(cli) => match cli.command {
            Some(Commands::Update) => Ok(CliAction::Update),
            Some(Commands::Check) => Ok(CliAction::Check),
            None => Ok(CliAction::Run),
        },
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            Ok(CliAction::PrintHelp(error.to_string()))
        }
        Err(error) => Err(error.into()),
    }
}

fn main() -> Result<()> {
    match parse_cli(std::env::args_os())? {
        CliAction::Run => {}
        CliAction::Update => {
            run_workspace_update()?;
            return Ok(());
        }
        CliAction::Check => {
            println!("{}", check_workspace_update(BUILD_COMMIT_HASH)?);
            return Ok(());
        }
        CliAction::PrintHelp(help) => {
            print!("{help}");
            return Ok(());
        }
    }

    let cfg = cmrt_runtime::Config::load()?;
    let realtime_cfg = RealtimeServerConfig::load()?;
    validate_realtime_play_server_config(&cfg, &realtime_cfg)?;

    let core_cfg = core_config_from_runtime(&cfg, &realtime_cfg);
    let player: Arc<dyn PlayerHandle> = Arc::new(RealtimePlayer::new(
        core_cfg,
        cfg.plugin_path.clone(),
        RenderOptions::new().with_preroll_ms(RENDER_PREROLL_MS),
    )?);

    let shutdown = Arc::new(AtomicBool::new(false));
    install_shutdown_handler(Arc::clone(&shutdown))?;

    run_realtime_play_server(realtime_cfg.realtime_play_server_port, shutdown, player)
}

fn install_shutdown_handler(shutdown: Arc<AtomicBool>) -> Result<()> {
    ctrlc::set_handler(move || {
        shutdown.store(true, Ordering::SeqCst);
    })
    .context("failed to install Ctrl-C handler")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_without_subcommand_runs_server() {
        assert_eq!(
            parse_cli(["clap-mml-realtime-play-server"]).unwrap(),
            CliAction::Run
        );
    }

    #[test]
    fn update_subcommand_returns_update_action() {
        assert_eq!(
            parse_cli(["clap-mml-realtime-play-server", "update"]).unwrap(),
            CliAction::Update
        );
    }

    #[test]
    fn check_subcommand_returns_check_action() {
        assert_eq!(
            parse_cli(["clap-mml-realtime-play-server", "check"]).unwrap(),
            CliAction::Check
        );
    }

    #[test]
    fn help_lists_self_update_commands_and_server_details() {
        let CliAction::PrintHelp(help) =
            parse_cli(["clap-mml-realtime-play-server", "--help"]).unwrap()
        else {
            panic!("expected help action");
        };

        assert!(help.contains("Commands:"));
        assert!(help.contains("update"));
        assert!(help.contains("check"));
        assert!(help.contains("POST /play"));
    }

    #[test]
    fn unknown_argument_returns_error() {
        let error = parse_cli(["clap-mml-realtime-play-server", "unknown"]).unwrap_err();

        assert!(error
            .to_string()
            .contains("unrecognized subcommand 'unknown'"));
    }
}
