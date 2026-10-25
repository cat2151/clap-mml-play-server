//! 実プラグインを 1 つ読んで測るサブコマンドの中身。
//!
//! どれもサーバーを起動せず、オーディオデバイスも開かない。config から
//! プラグイン一覧を引く点だけ本番と同じ経路を通す。

use anyhow::{anyhow, Context as _, Result};
use cmrt_core::{
    kind_for_patch, probe_plugin_capabilities, PatchBases, PatchVoicing, PluginKind,
    RealtimeRenderer,
};
use serde::Serialize;

use crate::apply_surge_data_home_for;
use crate::cli::ExpectedVoicing;
use crate::config::{
    core_config_from_server_config, validate_realtime_play_server_config, RealtimeServerConfig,
};
use crate::pitch_tracking;
use crate::player::plugin_kinds;

/// config から引けるプラグインのうち、patch 文字列の形が合うもの。
struct ProbeTarget {
    kinds: Vec<PluginKind>,
    kind: usize,
    bases: PatchBases,
}

impl ProbeTarget {
    fn load(patch: &str) -> Result<Self> {
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
        let kind = kind_for_patch(&kinds, 0, Some(patch)).map_err(|error| anyhow!(error))?;
        let bases = PatchBases::from_kinds(&kinds);
        Ok(Self { kinds, kind, bases })
    }

    fn kind(&self) -> &PluginKind {
        &self.kinds[self.kind]
    }
}

pub(crate) fn run_voicing_probe(
    patch: &str,
    previous_patch: Option<&str>,
    json: bool,
    expect: Option<ExpectedVoicing>,
) -> Result<()> {
    let target = ProbeTarget::load(patch)?;
    let (kinds, kind) = (&target.kinds, target.kind());
    let resolve_patch = |patch: &str| target.bases.resolve(patch);
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
            kind_for_patch(kinds, 0, Some(previous_patch)).map_err(|error| anyhow!(error))?;
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

#[derive(Serialize)]
struct PitchTrackingReport<'a> {
    patch: &'a str,
    tracks_keyboard: bool,
    observations: Vec<NoteFrequency>,
}

#[derive(Serialize)]
struct NoteFrequency {
    note: u8,
    frequency_hz: Option<f64>,
}

/// 1 音色を [`pitch_tracking::NOTES`] の各 note で鳴らし、音高がついてくるかを測る。
///
/// note ごとに新しいインスタンスを作るのは、前の note の余韻を次の窓へ持ち込まないため。
/// 録るのは CC1=0・velocity 100 で note-on してから約 500ms（note-off は送らない）。
pub(crate) fn run_pitch_tracking_probe(patch: &str, json: bool) -> Result<()> {
    let target = ProbeTarget::load(patch)?;
    let kind = target.kind();
    let mut core_cfg = kind.core_cfg.clone();
    core_cfg.sample_rate = f64::from(pitch_tracking::SAMPLE_RATE);
    core_cfg.buffer_size = 256;
    core_cfg.patch_path = Some(target.bases.resolve(patch));
    let entry = cmrt_core::load_entry(&kind.plugin_path)?;
    let mut observations = Vec::with_capacity(pitch_tracking::NOTES.len());
    for note in pitch_tracking::NOTES {
        let mut renderer = RealtimeRenderer::new(&core_cfg, &entry)
            .with_context(|| format!("plugin_path={} patch={patch}", kind.plugin_path))?;
        for _ in 0..4 {
            renderer.render_live_chunk(&[])?;
        }
        let mut samples = renderer.render_live_chunk(&[[0xb0, 1, 0], [0x90, note, 100]])?;
        // stereo interleaved なので SAMPLE_RATE 個で 500ms。
        while samples.len() < pitch_tracking::SAMPLE_RATE as usize {
            samples.extend(renderer.render_live_chunk(&[])?);
        }
        observations.push(pitch_tracking::observe(note, &samples));
    }
    let report = PitchTrackingReport {
        patch,
        tracks_keyboard: pitch_tracking::tracks_keyboard(&observations),
        observations: observations
            .iter()
            .map(|o| NoteFrequency {
                note: o.note,
                frequency_hz: o.frequency_hz,
            })
            .collect(),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "patch={} tracks_keyboard={} frequencies={:?}",
            patch,
            report.tracks_keyboard,
            report
                .observations
                .iter()
                .map(|o| (o.note, o.frequency_hz))
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}

/// CLAP の能力を実測して表にする（ADR 0001 / 0006 の根拠を取り直すための口）。
///
/// `plugin_path` を渡さなければ、config から引ける**載りうるプラグイン全部**を測る。
/// 対応プラグインを増やす前は、まだ config に無い CLAP を `--plugin-path` で名指しする。
pub(crate) fn run_capability_probe(
    plugin_path: Option<&str>,
    plugin_id: Option<&str>,
    json: bool,
) -> Result<()> {
    let targets = match plugin_path {
        Some(plugin_path) => vec![(plugin_path.to_string(), plugin_id.map(str::to_string))],
        None => {
            let cfg = cmrt_server_config::ServerConfig::load()?;
            let realtime_cfg = RealtimeServerConfig::load()?;
            validate_realtime_play_server_config(&cfg, &realtime_cfg)?;
            let core_cfg = core_config_from_server_config(&cfg, &realtime_cfg);
            plugin_kinds(&cfg, &core_cfg)
                .into_iter()
                .map(|kind| (kind.plugin_path, kind.core_cfg.plugin_id))
                .collect()
        }
    };
    // Surge を測るときだけデータディレクトリの絞り込みが要る。スレッドを作る前に呼ぶ
    // 制約（`std::env::set_var`）があるので、1 本目を読み始める前にまとめて済ませる。
    crate::apply_surge_data_home_for_paths(&targets);

    let mut reports = Vec::with_capacity(targets.len());
    for (plugin_path, plugin_id) in &targets {
        reports.push(probe_plugin_capabilities(
            plugin_path,
            plugin_id.as_deref(),
        )?);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
        return Ok(());
    }
    for report in &reports {
        println!(
            "{}
",
            report.to_text()
        );
    }
    Ok(())
}
