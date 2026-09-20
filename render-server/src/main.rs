mod http;
mod lifetime_guard;

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{Context as _, Result};
use clap::{error::ErrorKind, Parser, Subcommand};
use cmrt_core::{
    check_workspace_update, embedded_patch_ref, encode_wav_i16, kind_for_patch, load_entry,
    log_boot, log_boot_fatal, mml_render_stateless_with_effects, plugin_kinds,
    run_workspace_update, CoreConfig, EffectPlugins, PluginKind, RenderOptions,
};
use cmrt_server_config::ServerConfig;
use http::run_render_server;

const RENDER_PREROLL_MS: u64 = 100;
const REQUIRED_SAMPLE_RATE: f64 = 48_000.0;
const BUILD_COMMIT_HASH: &str = env!("BUILD_COMMIT_HASH");

/// 選ばれた plugin descriptor の起動ログを、worker 数によらず 1 度だけにする。
static DESCRIPTOR_LOGGED: std::sync::Once = std::sync::Once::new();

#[derive(Debug, PartialEq, Eq)]
enum CliAction {
    /// `config` を渡すと既定の置き場ではなくそのファイルを読む（診断用）。
    Run {
        config: Option<PathBuf>,
    },
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
    /// 既定の置き場ではなく、この config.toml を読む（診断用）
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

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
            None => Ok(CliAction::Run { config: cli.config }),
        },
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            Ok(CliAction::PrintHelp(error.to_string()))
        }
        Err(error) => Err(error.into()),
    }
}

/// config 由来の失敗は「このサーバーが起動できない理由」そのものなので、
/// anyhow で返すだけでなく boot ログにも 1 行残す。理由は realtime-play-server 側と同じ。
fn load_config(path: Option<&std::path::Path>) -> Result<ServerConfig> {
    let loaded = (|| {
        let cfg = match path {
            Some(path) => ServerConfig::load_from_path(path)?,
            None => ServerConfig::load()?,
        };
        validate_render_server_config(&cfg)?;
        Ok(cfg)
    })();
    loaded.inspect_err(|error: &anyhow::Error| log_boot_fatal("config", &format!("{error:#}")))
}

/// 既定の panic hook は stderr へ書くだけで終わる。CLAP plugin の `extern "C"` 境界を
/// 越えて unwind すると（Rust の規則で）即 abort になり、その前に書いた行が worker の
/// 出力バッファに埋もれて見えなくなることがある。ここで明示的に `flush` してから既定の
/// hook を呼び、クラッシュ調査でパニックか access violation かを切り分けられるようにする。
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }));
}

fn main() -> Result<()> {
    install_panic_hook();
    // 「どの実体を、どの版で起動したか」を、失敗しうる処理より前に残す。
    log_boot(BUILD_COMMIT_HASH);
    let config_path = match parse_cli(std::env::args_os())? {
        CliAction::Run { config } => config,
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
    };

    let cfg = load_config(config_path.as_deref())?;
    let core_cfg = core_config_from_server_config(&cfg);
    // 受け取る MML の音色がどのプラグインのものかは、リクエストが来るまで分からない。
    // 載りうるものを最初に全部並べておき、レンダリングのたびに patch 文字列で引き分ける
    // （`docs/adr/0007-patch-string-decides-the-plugin.md`）。
    let kinds = plugin_kinds(&cfg, &core_cfg);
    // `std::env::set_var` の制約で worker スレッド生成前に呼ぶ必要があるが、どのプラグインを
    // 使うかは config を読むまで分からないので、config のロード直後に置く。
    apply_surge_data_home_for(&kinds);
    let sample_rate = core_cfg.sample_rate as u32;
    let workers = cfg.offline_render_server_workers;
    // catalog の走査（preset ファイルの読み取り）は起動時に済ませ、最初の render を
    // 遅らせない。DLL のロードは chain 付きの MML を初めて受け取るまで遅らせる。
    let effect_plugins = EffectPlugins::discover();
    log_effect_catalog(&effect_plugins);

    let shutdown = Arc::new(AtomicBool::new(false));
    install_shutdown_handler(Arc::clone(&shutdown))?;
    lifetime_guard::install_if_requested(Arc::clone(&shutdown))?;

    run_render_server(
        cfg.offline_render_server_port,
        workers,
        shutdown,
        move || {
            let kinds = kinds.clone();
            // effect の catalog と entry 表はプロセスに 1 つを worker 間で共有する
            // （`Arc` で安い clone。instrument の entry のように worker ごとにロードし
            // 直す理由が無い）。
            let effect_plugins = effect_plugins.clone();
            // entry は worker ごとにロードする（今までどおり）。載りうるプラグインぶん
            // 並べるので、Dexed の音色を受け取っても worker を作り直さずに済む。
            let entries = kinds
                .iter()
                .map(|kind| load_entry(&kind.plugin_path))
                .collect::<Result<Vec<_>>>()?;
            // どのプラグインが選ばれたかは設定ミスを診断する唯一の手掛かりなので必ず出す。
            // 同じ行が並ばないよう worker 数によらず 1 度だけにする。
            // レンダリング側も同じ `core_cfg.plugin_id` で descriptor を選ぶので、
            // このログと実際に鳴るプラグインは必ず一致する。
            let mut descriptors = Vec::with_capacity(kinds.len());
            for (kind, entry) in kinds.iter().zip(&entries) {
                let descriptor =
                    cmrt_core::select_descriptor(entry, kind.core_cfg.plugin_id.as_deref())
                        .with_context(|| format!("plugin_path={}", kind.plugin_path))?;
                descriptors.push(format!(
                    "cmrt-render-server: plugin {} plugin_path={}",
                    descriptor.log_fields(),
                    kind.plugin_path
                ));
            }
            DESCRIPTOR_LOGGED.call_once(|| {
                for line in descriptors {
                    eprintln!("{line}");
                }
            });
            Ok(move |mml: &str| {
                // 音色無指定の MML は既定プラグイン（先頭）で鳴らす。
                let index = kind_for_patch(&kinds, 0, embedded_patch_ref(mml).as_deref())
                    .map_err(|error| anyhow::anyhow!(error))?;
                let samples = effect_plugins.with_render_effects(|effects| {
                    mml_render_stateless_with_effects(
                        mml,
                        &kinds[index].core_cfg,
                        &entries[index],
                        RenderOptions::new().with_preroll_ms(RENDER_PREROLL_MS),
                        effects,
                    )
                })?;
                encode_wav_i16(&samples, sample_rate)
            })
        },
    )
}

/// 載りうるプラグインの中に Surge XT があるなら、そのデータディレクトリを絞る。
///
/// 既定プラグインが Dexed でも、`.fxp` の音色を受け取れば Surge のインスタンスを作る。
/// `std::env::set_var` はスレッド生成前にしか呼べないので、既定プラグインだけを見て
/// 判断すると、あとから作る Surge インスタンスが絞り込みの恩恵を受けられない。
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
/// worker スレッドを spawn する前に呼ぶこと（`std::env::set_var` の制約）。
///
/// Surge XT 以外のプラグイン（Dexed 等）では、探しても見つからない Surge データの
/// 警告が出るだけなので実行しない。
fn apply_surge_data_home(plugin_id: Option<&str>, plugin_path: &str) {
    if !cmrt_core::plugin_is_surge(plugin_id, plugin_path) {
        eprintln!(
            "cmrt-render-server: surge_data_home skipped detail=Surge XT 以外のプラグインのため不要 plugin_path={plugin_path}"
        );
        return;
    }
    match cmrt_core::apply_minimal_surge_data_home() {
        Ok(setup) => eprintln!(
            "cmrt-render-server: surge_data_home rebuilt={} path={}",
            setup.rebuilt,
            setup.path.display()
        ),
        Err(error) => {
            eprintln!("cmrt-render-server: surge_data_home skipped detail={error:#}")
        }
    }
}

/// effect の catalog を先に走査しておき、件数を stderr へ残す。
/// `EffectPlugins::discover()` は必ず「chain 付きの MML を受け付ける」側で作るので、
/// catalog は常に `Some`。走査は `catalog()` の初回呼び出しでここに閉じ込める。
fn log_effect_catalog(effect_plugins: &EffectPlugins) {
    let Some(catalog) = effect_plugins.catalog() else {
        return;
    };
    eprintln!(
        "cmrt-render-server: effect-catalog: plugins={} presets={} skipped={}",
        catalog.plugins().len(),
        catalog.presets().len(),
        catalog.skipped().len()
    );
}

fn validate_render_server_config(cfg: &ServerConfig) -> Result<()> {
    if cfg.plugin_path.trim().is_empty() {
        anyhow::bail!("plugin_path が空です");
    }
    if cfg.sample_rate != REQUIRED_SAMPLE_RATE {
        anyhow::bail!("render-server は sample_rate = 48000 の config のみ対応します");
    }
    Ok(())
}

fn core_config_from_server_config(cfg: &ServerConfig) -> CoreConfig {
    CoreConfig {
        plugin_id: cfg.plugin_id.clone(),
        output_midi: cfg.output_midi.clone(),
        output_wav: cfg.output_wav.clone(),
        sample_rate: cfg.sample_rate,
        buffer_size: cfg.buffer_size,
        patch_path: None,
        patches_dir: cfg.patch_root_dir(),
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
mod tests;
