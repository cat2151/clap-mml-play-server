mod config;
mod http;
mod player;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::{Context as _, Result};
use cmrt_core::RenderOptions;
use config::{
    core_config_from_runtime, validate_realtime_play_server_config, RealtimeServerConfig,
};
use http::run_realtime_play_server;
use player::{PlayerHandle, RealtimePlayer};

const RENDER_PREROLL_MS: u64 = 100;

fn main() -> Result<()> {
    if help_requested()? {
        print_help();
        return Ok(());
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

fn help_requested() -> Result<bool> {
    let Some(arg) = std::env::args().nth(1) else {
        return Ok(false);
    };
    match arg.as_str() {
        "-h" | "--help" => Ok(true),
        _ => anyhow::bail!("unknown argument: {arg}"),
    }
}

fn print_help() {
    println!(
        "clap-mml-realtime-play-server\n\nUSAGE:\n    clap-mml-realtime-play-server\n\nCONFIG:\n    config_local_dir()/clap-mml-render-tui/config.toml\n\nHTTP:\n    GET /health\n    POST /play   request: Standard MIDI File bytes, Content-Type: audio/midi | audio/x-midi | application/octet-stream\n    POST /stop"
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn print_help_does_not_panic() {
        super::print_help();
    }
}
