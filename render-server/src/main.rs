mod http;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::{Context as _, Result};
use clap::{error::ErrorKind, Parser, Subcommand};
use cmrt_core::{
    check_workspace_update, encode_wav_i16, load_entry, mml_render_stateless_with_options,
    run_workspace_update, CoreConfig, RenderOptions,
};
use cmrt_runtime::Config;
use http::run_render_server;

const RENDER_PREROLL_MS: u64 = 100;
const REQUIRED_SAMPLE_RATE: f64 = 48_000.0;
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
    name = "clap-mml-render-server",
    about = "Render MML to WAV through a CLAP plugin",
    disable_help_subcommand = true,
    disable_version_flag = true,
    args_conflicts_with_subcommands = true,
    after_help = "CONFIG:\n    config_local_dir()/clap-mml-render-tui/config.toml\n\nHTTP:\n    POST /render\n    response: audio/wav, 16bit stereo 48000Hz"
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

    let cfg = Config::load()?;
    validate_render_server_config(&cfg)?;
    let core_cfg = core_config_from_runtime(&cfg);
    let plugin_path = cfg.plugin_path.clone();
    let sample_rate = core_cfg.sample_rate as u32;
    let workers = cfg.offline_render_server_workers;

    let shutdown = Arc::new(AtomicBool::new(false));
    install_shutdown_handler(Arc::clone(&shutdown))?;

    run_render_server(
        cfg.offline_render_server_port,
        workers,
        shutdown,
        move || {
            let core_cfg = core_cfg.clone();
            let entry = load_entry(&plugin_path)?;
            Ok(move |mml: &str| {
                let samples = mml_render_stateless_with_options(
                    mml,
                    &core_cfg,
                    &entry,
                    RenderOptions::new().with_preroll_ms(RENDER_PREROLL_MS),
                )?;
                encode_wav_i16(&samples, sample_rate)
            })
        },
    )
}

fn validate_render_server_config(cfg: &Config) -> Result<()> {
    if cfg.plugin_path.trim().is_empty() {
        anyhow::bail!("plugin_path が空です");
    }
    if cfg.sample_rate != REQUIRED_SAMPLE_RATE {
        anyhow::bail!("render-server は sample_rate = 48000 の config のみ対応します");
    }
    Ok(())
}

fn core_config_from_runtime(cfg: &Config) -> CoreConfig {
    CoreConfig {
        output_midi: cfg.output_midi.clone(),
        output_wav: cfg.output_wav.clone(),
        sample_rate: cfg.sample_rate,
        buffer_size: cfg.buffer_size,
        patch_path: None,
        patches_dir: cmrt_runtime::core_config_patch_root_dir(cfg),
        random_patch: false,
    }
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

    fn test_config() -> Config {
        Config {
            plugin_path: "plugin.clap".to_string(),
            input_midi: "input.mid".to_string(),
            output_midi: "output.mid".to_string(),
            output_wav: "output.wav".to_string(),
            sample_rate: REQUIRED_SAMPLE_RATE,
            buffer_size: 512,
            patches_dirs: None,
            offline_render_workers: cmrt_runtime::DEFAULT_OFFLINE_RENDER_WORKERS,
            offline_render_server_workers: cmrt_runtime::DEFAULT_OFFLINE_RENDER_SERVER_WORKERS,
            offline_render_backend: cmrt_runtime::OfflineRenderBackend::InProcess,
            offline_render_server_port: cmrt_runtime::DEFAULT_OFFLINE_RENDER_SERVER_PORT,
            offline_render_server_command: String::new(),
            realtime_audio_backend: cmrt_runtime::RealtimeAudioBackend::InProcess,
            realtime_play_server_port: cmrt_runtime::DEFAULT_REALTIME_PLAY_SERVER_PORT,
            realtime_play_server_command: String::new(),
            autoplay_on_startup: true,
        }
    }

    #[test]
    fn cli_without_subcommand_runs_server() {
        assert_eq!(
            parse_cli(["clap-mml-render-server"]).unwrap(),
            CliAction::Run
        );
    }

    #[test]
    fn update_subcommand_returns_update_action() {
        assert_eq!(
            parse_cli(["clap-mml-render-server", "update"]).unwrap(),
            CliAction::Update
        );
    }

    #[test]
    fn check_subcommand_returns_check_action() {
        assert_eq!(
            parse_cli(["clap-mml-render-server", "check"]).unwrap(),
            CliAction::Check
        );
    }

    #[test]
    fn help_lists_self_update_commands_and_server_details() {
        let CliAction::PrintHelp(help) = parse_cli(["clap-mml-render-server", "--help"]).unwrap()
        else {
            panic!("expected help action");
        };

        assert!(help.contains("Commands:"));
        assert!(help.contains("update"));
        assert!(help.contains("check"));
        assert!(help.contains("POST /render"));
    }

    #[test]
    fn unknown_argument_returns_error() {
        let error = parse_cli(["clap-mml-render-server", "unknown"]).unwrap_err();

        assert!(error
            .to_string()
            .contains("unrecognized subcommand 'unknown'"));
    }

    #[test]
    fn core_config_from_runtime_uses_cmrt_runtime_patch_root() {
        let mut cfg = test_config();
        cfg.patches_dirs = Some(vec![
            "/tmp/surge-data/patches_factory".to_string(),
            "/tmp/surge-data/patches_3rdparty".to_string(),
        ]);

        let core_cfg = core_config_from_runtime(&cfg);

        assert_eq!(core_cfg.output_midi, "output.mid");
        assert_eq!(core_cfg.output_wav, "output.wav");
        assert_eq!(core_cfg.sample_rate, REQUIRED_SAMPLE_RATE);
        assert_eq!(core_cfg.buffer_size, 512);
        assert_eq!(core_cfg.patches_dir.as_deref(), Some("/tmp/surge-data"));
        assert!(!core_cfg.random_patch);
    }

    #[test]
    fn validate_render_server_config_rejects_non_48khz() {
        let mut cfg = test_config();
        cfg.sample_rate = 44_100.0;

        let error = validate_render_server_config(&cfg).unwrap_err();

        assert!(error.to_string().contains("48000"));
    }
}
