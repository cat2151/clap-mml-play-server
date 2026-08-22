mod cli;
mod config;
mod fast_ipc;
mod http;
mod player;
mod probe;
mod timing;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use anyhow::{Context as _, Result};
use cli::{parse_cli, CliAction};
use cmrt_core::{
    check_workspace_update, log_boot, log_boot_fatal, run_workspace_update, RenderOptions,
};
use config::{
    core_config_from_server_config, validate_realtime_play_server_config, RealtimeServerConfig,
};
use http::run_realtime_play_server;
use player::{plugin_kinds, PlayerHandle, PluginKind, RealtimePlayer};
use probe::{run_capability_probe, run_voicing_probe};

const RENDER_PREROLL_MS: u64 = 100;
const BUILD_COMMIT_HASH: &str = env!("BUILD_COMMIT_HASH");

/// config 由来の失敗は「このサーバーが起動できない理由」そのものなので、
/// anyhow で返すだけでなく boot ログにも 1 行残す。
///
/// クライアントから見ると config エラーは「子プロセスが即死した」だけで、
/// 何が悪いのかは stderr を読むまで分からない。プレフィックス付きの 1 行にしておけば、
/// `cmrt-server-boot:` で grep したときに起動した版と失敗理由が並んで出る。
fn load_configs() -> Result<(cmrt_server_config::ServerConfig, RealtimeServerConfig)> {
    let loaded = (|| {
        let cfg = cmrt_server_config::ServerConfig::load()?;
        let realtime_cfg = RealtimeServerConfig::load()?;
        validate_realtime_play_server_config(&cfg, &realtime_cfg)?;
        Ok((cfg, realtime_cfg))
    })();
    loaded.inspect_err(|error: &anyhow::Error| log_boot_fatal("config", &format!("{error:#}")))
}

fn main() -> Result<()> {
    // 起動時間計測の基準点。以降 timing::log() が「起動から何 ms か」を添える。
    timing::boot();
    // 「どの実体を、どの版で起動したか」を、失敗しうる処理より前に残す。
    log_boot(BUILD_COMMIT_HASH);
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
        CliAction::ProbeCapabilities {
            plugin_path,
            plugin_id,
            json,
        } => {
            run_capability_probe(plugin_path.as_deref(), plugin_id.as_deref(), json)?;
            return Ok(());
        }
        CliAction::PrintHelp(help) => {
            print!("{help}");
            return Ok(());
        }
    }

    let config_started = Instant::now();
    let (cfg, realtime_cfg) = load_configs()?;
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
pub(crate) fn apply_surge_data_home_for(kinds: &[PluginKind]) {
    let targets: Vec<(String, Option<String>)> = kinds
        .iter()
        .map(|kind| (kind.plugin_path.clone(), kind.core_cfg.plugin_id.clone()))
        .collect();
    apply_surge_data_home_for_paths(&targets);
}

/// [`apply_surge_data_home_for`] と同じ判断を、まだ `PluginKind` になっていない
/// (plugin_path, plugin_id) の組へ行う。
///
/// capability probe は config に載っていない CLAP も名指しで測るので、`PluginKind` を
/// 組めない。判断そのものは 1 か所に残したいのでこちらを実体にしてある。
pub(crate) fn apply_surge_data_home_for_paths(targets: &[(String, Option<String>)]) {
    let surge = targets
        .iter()
        .find(|(plugin_path, plugin_id)| {
            cmrt_core::plugin_is_surge(plugin_id.as_deref(), plugin_path)
        })
        // Surge が 1 つも無い構成。ログの体裁を既定プラグインで揃えるためだけに渡す。
        .or_else(|| targets.first());
    if let Some((plugin_path, plugin_id)) = surge {
        apply_surge_data_home(plugin_id.as_deref(), plugin_path);
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

fn install_shutdown_handler(shutdown: Arc<AtomicBool>) -> Result<()> {
    ctrlc::set_handler(move || {
        shutdown.store(true, Ordering::SeqCst);
    })
    .context("failed to install Ctrl-C handler")
}
