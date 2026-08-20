mod config;
mod fast_ipc;
mod http;
mod player;
mod timing;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use anyhow::{anyhow, Context as _, Result};
use clap::{error::ErrorKind, Parser, Subcommand, ValueEnum};
use cmrt_core::{
    check_workspace_update, kind_for_patch, run_workspace_update, PatchBases, PatchVoicing,
    RealtimeRenderer, RenderOptions,
};
use config::{
    core_config_from_server_config, validate_realtime_play_server_config, RealtimeServerConfig,
};
use http::run_realtime_play_server;
use player::{plugin_kinds, PlayerHandle, PluginKind, RealtimePlayer};

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
    after_help = "CONFIG:\n    config_local_dir()/clap-mml-render-tui/config.toml\n\nHTTP:\n    GET /health\n    POST /play               request: Standard MIDI File bytes, Content-Type: audio/midi | audio/x-midi | application/octet-stream\n    POST /play-mml           request: MML text (leading {\"Surge XT patch\": ...} JSON selects the patch), Content-Type: text/plain\n    POST /stop\n\nLIVE MIDI (Windows):\n    1/2/4/8/16/32 CLAP instances through named shared memory on the realtime server port\n    CMRT_LIVE_INSTANCE_COUNT selects the count (default: 16)\n    32 is for the grid sequencer chord mode, which double-buffers 16 tracks across two banks"
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
    // 起動時間計測の基準点。以降 timing::log() が「起動から何 ms か」を添える。
    timing::boot();
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

    let config_started = Instant::now();
    let cfg = cmrt_server_config::ServerConfig::load()?;
    let realtime_cfg = RealtimeServerConfig::load()?;
    validate_realtime_play_server_config(&cfg, &realtime_cfg)?;
    timing::log_phase("config", config_started.elapsed());

    let core_cfg = core_config_from_server_config(&cfg, &realtime_cfg);
    // 1 プロセスに複数のプラグインを載せうるので、Surge データディレクトリの判定も
    // 「載りうるものの中に Surge があるか」で行う。
    let kinds = plugin_kinds(&cfg, &core_cfg);
    apply_surge_data_home_for(&kinds);

    let player: Arc<dyn PlayerHandle> = Arc::new(RealtimePlayer::new(
        core_cfg,
        kinds,
        RenderOptions::new().with_preroll_ms(RENDER_PREROLL_MS),
        realtime_cfg.live_instance_count,
    )?);

    let shutdown = Arc::new(AtomicBool::new(false));
    install_shutdown_handler(Arc::clone(&shutdown))?;

    run_realtime_play_server(realtime_cfg.realtime_play_server_port, shutdown, player)
}

/// 載りうるプラグインの中に Surge XT があるなら、そのデータディレクトリを絞る。
///
/// 予備インスタンスプールは Surge を「既定プラグインではないが背景で作りうるもの」として
/// 持ちうる。`std::env::set_var` はスレッド生成前にしか呼べないので、既定プラグインだけを
/// 見て判断すると、あとから作る Surge インスタンスが絞り込みの恩恵を受けられない。
fn apply_surge_data_home_for(kinds: &[PluginKind]) {
    let surge = kinds.iter().find(|kind| {
        cmrt_core::plugin_is_surge(kind.core_cfg.plugin_id.as_deref(), &kind.plugin_path)
    });
    match surge {
        Some(kind) => apply_surge_data_home(kind.core_cfg.plugin_id.as_deref(), &kind.plugin_path),
        // Surge が 1 つも無い構成。ログの体裁を既定プラグインで揃えるためだけに渡す。
        None => apply_surge_data_home(
            kinds[0].core_cfg.plugin_id.as_deref(),
            &kinds[0].plugin_path,
        ),
    }
}

/// Surge XT のデータディレクトリを最小構成へ向けて `init()` を速くする。
///
/// 失敗しても環境変数を設定しないだけで、Surge の既定動作のまま起動できる。
/// スレッドを spawn する前に呼ぶこと（`std::env::set_var` の制約）。
///
/// Surge XT 以外のプラグイン（Dexed 等）では、探しても見つからない Surge データの
/// 警告が出るだけなので実行しない。
fn apply_surge_data_home(plugin_id: Option<&str>, plugin_path: &str) {
    let started = Instant::now();
    let ms = |started: Instant| started.elapsed().as_millis();
    if !cmrt_core::plugin_is_surge(plugin_id, plugin_path) {
        timing::log(&format!(
            "phase=surge_data_home ms={} result=skipped detail=Surge XT 以外のプラグインのため不要 plugin_path={plugin_path}",
            ms(started)
        ));
        return;
    }
    match cmrt_core::apply_minimal_surge_data_home() {
        Ok(setup) => timing::log(&format!(
            "phase=surge_data_home ms={} result=ok rebuilt={} path={}",
            ms(started),
            setup.rebuilt,
            setup.path.display()
        )),
        Err(error) => timing::log(&format!(
            "phase=surge_data_home ms={} result=skipped detail={error:#}",
            ms(started)
        )),
    }
}

fn run_voicing_probe(
    patch: &str,
    previous_patch: Option<&str>,
    json: bool,
    expect: Option<ExpectedVoicing>,
) -> Result<()> {
    let cfg = cmrt_server_config::ServerConfig::load()?;
    let realtime_cfg = RealtimeServerConfig::load()?;
    validate_realtime_play_server_config(&cfg, &realtime_cfg)?;
    let mut core_cfg = core_config_from_server_config(&cfg, &realtime_cfg);
    core_cfg.patch_path = None;
    // probe する音色がどのプラグインのものかは patch 文字列の形で決まる。既定プラグインで
    // 決め打つと、Dexed を既定にした環境で `.fxp` を probe できない（逆も同じ）。
    let kinds = plugin_kinds(&cfg, &core_cfg);
    // `std::env::set_var` の制約でスレッド生成前に呼ぶ必要があるが、どのプラグインを
    // 使うかは config を読むまで分からないので、config のロード直後に置く。
    apply_surge_data_home_for(&kinds);
    let kind = &kinds[kind_for_patch(&kinds, 0, Some(patch)).map_err(|error| anyhow!(error))?];
    let bases = PatchBases::from_kinds(&kinds);
    let resolve_patch = |patch: &str| match (
        bases.base_for(patch),
        std::path::Path::new(patch).is_absolute(),
    ) {
        (_, true) | (None, false) => patch.to_string(),
        (Some(base), false) => std::path::Path::new(base)
            .join(patch)
            .to_string_lossy()
            .into_owned(),
    };
    let patch_path = resolve_patch(patch);
    let entry = cmrt_core::load_entry(&kind.plugin_path)?;
    // 下の `RealtimeRenderer::new` も同じ `core_cfg.plugin_id` で descriptor を選ぶ。
    let descriptor = cmrt_core::select_descriptor(&entry, kind.core_cfg.plugin_id.as_deref())
        .with_context(|| format!("plugin_path={}", kind.plugin_path))?;
    eprintln!("plugin: {}", descriptor.log_fields());
    let mut renderer = RealtimeRenderer::new(&kind.core_cfg, &entry)
        .with_context(|| format!("plugin_path={}", kind.plugin_path))?;
    if let Some(previous_patch) = previous_patch {
        // 1 つのインスタンスで両方を鳴らす probe なので、プラグインをまたぐ組み合わせは
        // 成立しない。黙って別プラグインの音色を読ませると「操作は成功したが前の音のまま」
        // になるため、ここで落とす。
        let previous_kind =
            kind_for_patch(&kinds, 0, Some(previous_patch)).map_err(|error| anyhow!(error))?;
        if kinds[previous_kind].plugin_path != kind.plugin_path {
            anyhow::bail!(
                "--previous-patch はプラグインをまたげません: '{previous_patch}' は {} / '{patch}' は {}",
                kinds[previous_kind].name,
                kind.name
            );
        }
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
