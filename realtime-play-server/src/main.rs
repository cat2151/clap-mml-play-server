mod config;
mod fast_ipc;
mod http;
mod player;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::{Context as _, Result};
use clap::{error::ErrorKind, Parser, Subcommand, ValueEnum};
use cmrt_core::{
    check_workspace_update, run_workspace_update, PatchVoicing, RealtimeRenderer, RenderOptions,
};
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
    ProbeVoicing {
        patch: String,
        previous_patch: Option<String>,
        json: bool,
        expect: Option<ExpectedVoicing>,
    },
    PrintHelp(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum ExpectedVoicing {
    Mono,
    Poly,
    Unknown,
}

impl From<ExpectedVoicing> for PatchVoicing {
    fn from(value: ExpectedVoicing) -> Self {
        match value {
            ExpectedVoicing::Mono => Self::Mono,
            ExpectedVoicing::Poly => Self::Poly,
            ExpectedVoicing::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "clap-mml-realtime-play-server",
    about = "Play Standard MIDI Files through a CLAP plugin",
    disable_help_subcommand = true,
    disable_version_flag = true,
    args_conflicts_with_subcommands = true,
    after_help = "CONFIG:\n    config_local_dir()/clap-mml-render-tui/config.toml\n\nHTTP:\n    GET /health\n    POST /play               request: Standard MIDI File bytes, Content-Type: audio/midi | audio/x-midi | application/octet-stream\n    POST /play-mml           request: MML text (leading {\"Surge XT patch\": ...} JSON selects the patch), Content-Type: text/plain\n    POST /stop\n\nLIVE MIDI (Windows):\n    16 CLAP instances through named shared memory on the realtime server port"
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
    /// Probe a patch for mono/poly voicing without opening an audio device
    ProbeVoicing {
        /// Patch path relative to patches_dir, or an absolute path
        #[arg(long)]
        patch: String,
        /// Probe this patch first on the same plugin instance
        #[arg(long)]
        previous_patch: Option<String>,
        /// Print the complete report as JSON
        #[arg(long)]
        json: bool,
        /// Fail unless the final decision matches this value
        #[arg(long, value_enum)]
        expect: Option<ExpectedVoicing>,
    },
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
            Some(Commands::ProbeVoicing {
                patch,
                previous_patch,
                json,
                expect,
            }) => Ok(CliAction::ProbeVoicing {
                patch,
                previous_patch,
                json,
                expect,
            }),
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
        CliAction::ProbeVoicing {
            patch,
            previous_patch,
            json,
            expect,
        } => {
            run_voicing_probe(&patch, previous_patch.as_deref(), json, expect)?;
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

fn run_voicing_probe(
    patch: &str,
    previous_patch: Option<&str>,
    json: bool,
    expect: Option<ExpectedVoicing>,
) -> Result<()> {
    let cfg = cmrt_runtime::Config::load()?;
    let realtime_cfg = RealtimeServerConfig::load()?;
    validate_realtime_play_server_config(&cfg, &realtime_cfg)?;
    let mut core_cfg = core_config_from_runtime(&cfg, &realtime_cfg);
    core_cfg.patch_path = None;
    let resolve_patch = |patch: &str| match (
        &core_cfg.patches_dir,
        std::path::Path::new(patch).is_absolute(),
    ) {
        (_, true) | (None, false) => patch.to_string(),
        (Some(base), false) => std::path::Path::new(base)
            .join(patch)
            .to_string_lossy()
            .into_owned(),
    };
    let patch_path = resolve_patch(patch);
    let entry = cmrt_core::load_entry(&cfg.plugin_path)?;
    let mut renderer = RealtimeRenderer::new(&core_cfg, &entry)?;
    if let Some(previous_patch) = previous_patch {
        renderer.set_patch(Some(&resolve_patch(previous_patch)))?;
        let _ = renderer.probe_voicing()?;
    }
    renderer.set_patch(Some(&patch_path))?;
    let report = renderer.probe_voicing()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "patch={} decision={:?} probe={:?} ended_note_ids={:?} disagreement={}",
            patch,
            report.decision,
            report.probe.result,
            report.probe.ended_note_ids,
            report.disagreement
        );
    }
    if let Some(expect) = expect {
        let expected = PatchVoicing::from(expect);
        if report.decision != expected {
            anyhow::bail!(
                "voicing expectation failed: expected {:?}, got {:?}",
                expected,
                report.decision
            );
        }
    }
    Ok(())
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
    fn probe_voicing_subcommand_parses_automation_options() {
        assert_eq!(
            parse_cli([
                "clap-mml-realtime-play-server",
                "probe-voicing",
                "--patch",
                "Leads/Mono.fxp",
                "--previous-patch",
                "Pads/Poly.fxp",
                "--json",
                "--expect",
                "mono",
            ])
            .unwrap(),
            CliAction::ProbeVoicing {
                patch: "Leads/Mono.fxp".into(),
                previous_patch: Some("Pads/Poly.fxp".into()),
                json: true,
                expect: Some(ExpectedVoicing::Mono),
            }
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
        assert!(help.contains("probe-voicing"));
        assert!(help.contains("POST /play"));
        assert!(help.contains("POST /play-mml"));
        assert!(help.contains("LIVE MIDI (Windows)"));
    }

    #[test]
    fn unknown_argument_returns_error() {
        let error = parse_cli(["clap-mml-realtime-play-server", "unknown"]).unwrap_err();

        assert!(error
            .to_string()
            .contains("unrecognized subcommand 'unknown'"));
    }
}
