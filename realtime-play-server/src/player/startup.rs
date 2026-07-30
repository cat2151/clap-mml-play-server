use std::time::Instant;

use cmrt_core::{CoreConfig, RealtimeRenderer, RendererCreated};

use crate::timing;

/// インスタンス生成に使うスレッド数を明示指定する環境変数。
const BUILD_THREADS_ENV: &str = "CMRT_INSTANCE_BUILD_THREADS";

pub(super) fn create_live_renderers(
    core_cfg: &CoreConfig,
    plugin_path: &str,
    instance_count: usize,
) -> anyhow::Result<Vec<RealtimeRenderer>> {
    let load_entry_started = Instant::now();
    let entry = cmrt_core::load_entry(plugin_path)?;
    timing::log_phase("load_entry", load_entry_started.elapsed());

    let instances_started = Instant::now();
    let threads = instance_build_threads(instance_count);
    let renderers = cmrt_core::create_renderers_parallel(
        core_cfg,
        &entry,
        instance_count,
        threads,
        &log_instance_created,
    )?;
    timing::log(&format!(
        "phase=instances_total ms={} count={instance_count} threads={threads}",
        instances_started.elapsed().as_millis(),
    ));
    Ok(renderers)
}

/// CLAP インスタンス1つ分の生成内訳と起動進捗を出力する。
fn log_instance_created(created: RendererCreated) {
    // クライアントがパースしている行。書式を変えないこと。
    eprintln!(
        "cmrt-server-startup: instances={}/{}",
        created.completed, created.total
    );
    let timing = created.timing;
    timing::log(&format!(
        "phase=instance index={} ms={} plugin_id_ms={} instantiate_ms={} \
         save_state_ms={} load_patch_ms={} activate_ms={} start_processing_ms={}",
        created.index,
        timing.total.as_millis(),
        timing.plugin_id.as_millis(),
        timing.instantiate.as_millis(),
        timing.save_state.as_millis(),
        timing.load_patch.as_millis(),
        timing.activate.as_millis(),
        timing.start_processing.as_millis(),
    ));
}

/// 生成数より多くのスレッドを作らない。
fn instance_build_threads(instance_count: usize) -> usize {
    let default = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    std::env::var(BUILD_THREADS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|threads| *threads > 0)
        .unwrap_or(default)
        .min(instance_count)
}
