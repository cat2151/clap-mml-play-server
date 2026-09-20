//! Surge XT Effects（`org.surge-synth-team.surge-xt-fx`）を `EffectRenderer` で host する実測。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::surge_fx_preset::juce_xml;
use crate::surge_fx_preset::{
    calibration_state_xml, param_layout, param_remap, parse_srgfx, parse_state_xml,
    SurgeFxParamRanges, SurgeFxSnapshot, SurgeFxStateReport, SURGE_FX_PARAM_COUNT,
    SURGE_FX_PARAM_LAYOUTS, SURGE_FX_PLUGIN_ID, VALTYPE_INT,
};

const SURGEFX_CLAP_ENV: &str = "CMRT_TEST_SURGEFX_CLAP";
const SURGEFX_PRESETS_ENV: &str = "CMRT_TEST_SURGEFX_PRESETS";

fn surge_fx_renderer() -> EffectRenderer {
    let entry = load_entry(&plugin_path(SURGEFX_CLAP_ENV)).unwrap();
    EffectRenderer::new(&entry, SURGE_FX_PLUGIN_ID, SAMPLE_RATE, BUFFER_SIZE).unwrap()
}

fn preset_path(relative: &str) -> PathBuf {
    Path::new(&plugin_path(SURGEFX_PRESETS_ENV)).join(relative)
}

fn load_snapshot_file(relative: &str) -> Vec<SurgeFxSnapshot> {
    let path = preset_path(relative);
    let xml = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    parse_srgfx(&xml).unwrap_or_else(|error| panic!("{}: {error:#}", path.display()))
}

fn saved_report(renderer: &mut EffectRenderer) -> SurgeFxStateReport {
    let saved = renderer.save_state().unwrap();
    parse_state_xml(&juce_xml::decode(&saved).unwrap()).unwrap()
}

/// snapshot を載せ、自己申告が preset の実値と一致することを確かめる。
fn load_and_check(
    renderer: &mut EffectRenderer,
    snapshot: &SurgeFxSnapshot,
    ranges: &SurgeFxParamRanges,
    label: &str,
) -> SurgeFxStateReport {
    renderer
        .load_surge_fx_snapshot(snapshot)
        .unwrap_or_else(|error| panic!("{label}: {error:#}"));
    let report = saved_report(renderer);
    assert_eq!(report.fx_type, snapshot.fx_type, "{label}: fxt");
    let layout = param_layout(snapshot.fx_type).unwrap();
    let remap = param_remap(layout);
    for (gui_index, storage_index) in remap.into_iter().enumerate() {
        if !layout.active[storage_index] {
            continue;
        }
        let raw = snapshot.params[storage_index].raw_value();
        let actual = report.value[gui_index];
        // plugin は state の値を `bound_value()` で範囲内へ丸める。Surge 自身の preset loader は
        // 丸めないが、範囲外の値は DSP 側で同じ範囲へ limit されるので出音は同じ。
        let (low, high) = (
            ranges.at_zero[gui_index].min(ranges.at_one[gui_index]),
            ranges.at_zero[gui_index].max(ranges.at_one[gui_index]),
        );
        let (expected, tolerance) = if report.valtype[gui_index] == VALTYPE_INT {
            (raw, 0.0)
        } else {
            let clamped = raw.clamp(low, high);
            (clamped, 1e-3 * clamped.abs().max(1.0))
        };
        assert!(
            (actual - expected).abs() <= tolerance,
            "{label}: p{storage_index} (GUI {gui_index}, valtype {}, range {low}..{high}) raw {raw} expected {expected} got {actual}",
            report.valtype[gui_index]
        );
        // temposync / extend は plugin がそのまま返す。deactivated は他の parameter に
        // 連動して plugin 側が立てることがあるので、preset が立てた bit が残っていることだけを見る。
        let param = &snapshot.params[storage_index];
        let expected_features = i32::from(param.temposync)
            | (i32::from(param.extend_range) << 1)
            | (i32::from(param.deactivated) << 3);
        let reported = report.features[gui_index];
        assert_eq!(
            reported & 0b0011,
            expected_features & 0b0011,
            "{label}: p{storage_index} (GUI {gui_index}) の temposync/extend bit"
        );
        assert_eq!(
            reported & expected_features & 0b1000,
            expected_features & 0b1000,
            "{label}: p{storage_index} (GUI {gui_index}) の deactivated bit (reported {reported})"
        );
    }
    report
}

/// `process` を block ごとに回す（latency 補正なし）。
fn process_blocks(renderer: &mut EffectRenderer, samples: &mut [f32]) {
    renderer.ensure_active().unwrap();
    for block in samples.as_chunks_mut::<{ BUFFER_SIZE * 2 }>().0.iter_mut() {
        renderer.process(block, None).unwrap();
    }
}

/// 10 ms のノイズバーストのあと無音を `seconds` 秒ぶん通す。
fn noise_burst_response(renderer: &mut EffectRenderer, seconds: usize) -> Vec<f32> {
    let frames = (SAMPLE_RATE as usize * seconds).next_multiple_of(BUFFER_SIZE);
    let mut samples = vec![0.0_f32; frames * 2];
    let burst = SAMPLE_RATE as usize / 100;
    let mut seed: u32 = 0x1234_5678;
    for sample in samples.iter_mut().take(burst * 2) {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *sample = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
    }
    process_blocks(renderer, &mut samples);
    samples
}

#[test]
#[ignore = "実 Surge XT Effects CLAP が要る"]
fn surge_fx_init_state_is_juce_xml_with_effect_off() {
    let started = std::time::Instant::now();
    let mut renderer = surge_fx_renderer();
    let instantiate = started.elapsed();
    let xml = juce_xml::decode(renderer.init_state()).unwrap();
    eprintln!("init state ({} byte): {xml}", renderer.init_state().len());
    let report = parse_state_xml(&xml).unwrap();
    assert_eq!(report.fx_type, 0);
    assert_eq!(renderer.input_port_channels(), vec![2, 2]);
    let started = std::time::Instant::now();
    renderer.ensure_active().unwrap();
    let activate = started.elapsed();
    let latency = renderer.latency();
    let names = renderer.param_names();
    eprintln!("instantiate={instantiate:?} activate+start={activate:?} latency={latency} params={names:?}");
    assert_eq!(names.len(), SURGE_FX_PARAM_COUNT + 1);
}

/// Delay を mix 0 にした snapshot（dry だけ通る）。latency 補正の物差しにする。
const DRY_DELAY_SRGFX: &str = r#"<single-fx streaming_version="17"><snapshot type="1" name="dry" p0="-3.0" p1="-3.0" p2="0.0" p3="0.0" p4="-60.0" p5="70.0" p6="-7.0" p7="0.0" p8="0.0" p10="0.0" p11="0.0"/></single-fx>"#;

/// `output[f] ≈ input[f - lag]` が最もよく成り立つ lag と、そのときの最大誤差。
fn best_lag(output: &[f32], input: &[f32], lags: std::ops::RangeInclusive<i64>) -> (i64, f32) {
    let frames = (input.len() / 2) as i64;
    lags.map(|lag| {
        let max_diff = (0..frames)
            .filter(|frame| (0..frames).contains(&(frame - lag)))
            .map(|frame| {
                let source = ((frame - lag) * 2) as usize;
                let target = (frame * 2) as usize;
                (output[target] - input[source])
                    .abs()
                    .max((output[target + 1] - input[source + 1]).abs())
            })
            .fold(0.0_f32, f32::max);
        (lag, max_diff)
    })
    .min_by(|a, b| a.1.total_cmp(&b.1))
    .unwrap()
}

/// dry な Delay を通した出力は、素の `process` では `clap.latency` ぶん遅れ、
/// `EffectChain::process_all` なら入力と同じ位置に戻る。
///
/// Surge XT Effects の latent mode（user default `fxAssumeFixedBlock=0`）は申告 32 に対して
/// 実際の遅れが 31 で、1 sample 食い違う。chain は申告値を信じるので残差は段数ぶんまで許す。
#[test]
#[ignore = "実 Surge XT Effects CLAP が要る"]
fn surge_fx_chain_aligns_output_with_input() {
    let frames = BUFFER_SIZE * 4;
    let mut input = vec![0.0_f32; frames * 2];
    for frame in BUFFER_SIZE..frames {
        let value = ((frame as f32) * 0.37).sin() * 0.1;
        input[frame * 2] = value;
        input[frame * 2 + 1] = -value;
    }
    let impulse_at = BUFFER_SIZE + 100;
    input[impulse_at * 2] = 0.5;
    input[impulse_at * 2 + 1] = -0.5;
    let dry_delay = parse_srgfx(DRY_DELAY_SRGFX).unwrap().remove(0);

    let mut raw = surge_fx_renderer();
    raw.load_surge_fx_snapshot(&dry_delay).unwrap();
    let mut delayed = input.clone();
    process_blocks(&mut raw, &mut delayed);
    let latency = i64::from(raw.latency());
    let (raw_lag, raw_diff) = best_lag(&delayed, &input, 0..=64);
    eprintln!("raw: clap.latency={latency} measured lag={raw_lag} max diff={raw_diff}");
    assert!(raw_diff < 1e-2, "dry Delay が透過でない: {raw_diff}");
    assert!(
        (latency - 1..=latency).contains(&raw_lag),
        "実際の遅れ {raw_lag} が申告 {latency} と合わない"
    );

    for stage_count in [1_i64, 2] {
        let stages: Vec<EffectRenderer> = (0..stage_count)
            .map(|_| {
                let mut stage = surge_fx_renderer();
                stage.load_surge_fx_snapshot(&dry_delay).unwrap();
                stage
            })
            .collect();
        let mut chain = EffectChain::new(stages).unwrap();
        let mut aligned = input.clone();
        chain
            .process_all(&mut aligned, EffectTransport::default())
            .unwrap();
        let chain_latency = i64::from(chain.latency());
        let (chain_lag, chain_diff) = best_lag(&aligned, &input, -8..=8);
        eprintln!(
            "{stage_count} stage(s): chain latency={chain_latency} measured lag={chain_lag} max diff={chain_diff}"
        );
        assert_eq!(chain_latency, latency * stage_count);
        assert_eq!(aligned.len(), input.len());
        assert!(
            chain_diff < 1e-2,
            "chain の出力が入力と一致しない: {chain_diff}"
        );
        assert_eq!(
            raw_lag * stage_count - chain_lag,
            chain_latency,
            "補正量が clap.latency の合計と違う"
        );
        assert!(
            chain_lag.abs() <= stage_count,
            "残差が段数を超えた: {chain_lag}"
        );
    }
}

/// Reverb 1 / Cathedral 2: 自己申告が一致し、ノイズバーストの尻尾が 0.5〜2.0 s に残る。
/// init state（effect off）では同区間が無音。
#[test]
#[ignore = "実 Surge XT Effects CLAP と factory preset が要る"]
fn surge_fx_cathedral_2_matches_self_report_and_leaves_a_reverb_tail() {
    let mut renderer = surge_fx_renderer();
    let init = renderer.init_state().to_vec();
    let snapshot = load_snapshot_file("Reverb 1/Cathedral 2.srgfx")
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(snapshot.fx_type, 2);
    let ranges = renderer.measure_surge_fx_ranges(snapshot.fx_type).unwrap();
    let started = std::time::Instant::now();
    load_and_check(&mut renderer, &snapshot, &ranges, "Cathedral 2");
    eprintln!("load_surge_fx_snapshot+save = {:?}", started.elapsed());

    let wet = noise_burst_response(&mut renderer, 3);
    eprintln!("latency after activate = {}", renderer.latency());
    let half = SAMPLE_RATE as usize / 2;
    let two = SAMPLE_RATE as usize * 2;
    let wet_tail = rms_dbfs_frames(&wet, half, two);

    renderer.load_state(&init).unwrap();
    let dry = noise_burst_response(&mut renderer, 3);
    let dry_tail = rms_dbfs_frames(&dry, half, two);
    eprintln!("tail RMS 0.5-2.0s: wet={wet_tail:.1} dBFS dry={dry_tail:.1} dBFS");
    assert!(wet_tail > -60.0, "reverb tail が薄すぎる: {wet_tail} dBFS");
    assert!(
        dry_tail < -90.0,
        "effect off の尻尾が無音でない: {dry_tail} dBFS"
    );
}

/// tempo-sync を持つ Delay preset でも自己申告が一致する（feature bit が state で通る）。
#[test]
#[ignore = "実 Surge XT Effects CLAP と factory preset が要る"]
fn surge_fx_temposync_delay_preset_matches_self_report() {
    let mut renderer = surge_fx_renderer();
    let snapshot = load_snapshot_file("Delay/Basic 1-16.srgfx")
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(snapshot.fx_type, 1);
    assert!(snapshot.params[0].temposync && snapshot.params[1].temposync);
    let ranges = renderer.measure_surge_fx_ranges(snapshot.fx_type).unwrap();
    let report = load_and_check(&mut renderer, &snapshot, &ranges, "Basic 1-16");
    let remap = param_remap(param_layout(1).unwrap());
    for (gui_index, storage_index) in remap.into_iter().enumerate() {
        let expected = i32::from(snapshot.params[storage_index].temposync);
        assert_eq!(
            report.features[gui_index] & 1,
            expected,
            "p{storage_index} の temposync bit"
        );
    }
}

/// 生成した layout の並びが、plugin が `clap.params` で名乗る名前と一致する。
///
/// state だけでは並びの取り違えが見えない（自己申告は並び替え後の値をそのまま返す）
/// ので、名前で突き合わせる。
#[test]
#[ignore = "実 Surge XT Effects CLAP が要る"]
fn surge_fx_param_names_agree_with_generated_layout() {
    let mut renderer = surge_fx_renderer();
    let mut checked = 0;
    for layout in SURGE_FX_PARAM_LAYOUTS {
        renderer
            .surge_fx_self_report(&calibration_state_xml(layout.fx_type, 0.0))
            .unwrap();
        let names = renderer.param_names();
        let remap = param_remap(layout);
        for (gui_index, storage_index) in remap.into_iter().enumerate() {
            let expected = layout.names[storage_index];
            if !layout.active[storage_index] || expected.is_empty() {
                continue;
            }
            let actual = &names[gui_index];
            assert!(
                actual == expected || actual.ends_with(&format!(" {expected}")),
                "fxt={} ({}): GUI {gui_index} は '{actual}'、layout は storage {storage_index} '{expected}'\n全名前: {names:?}",
                layout.fx_type,
                layout.display_name
            );
            checked += 1;
        }
    }
    eprintln!("checked {checked} parameter names");
    assert!(checked > 200);
}

/// factory preset の全 snapshot を載せ、自己申告が一致する。
#[test]
#[ignore = "実 Surge XT Effects CLAP と factory preset が要る"]
fn surge_fx_every_factory_snapshot_matches_self_report() {
    let root = PathBuf::from(plugin_path(SURGEFX_PRESETS_ENV));
    let mut files = Vec::new();
    collect_srgfx(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "{} に .srgfx が無い", root.display());

    let mut renderer = surge_fx_renderer();
    let mut ranges_by_type: HashMap<i32, SurgeFxParamRanges> = HashMap::new();
    let mut snapshots = 0;
    let mut skipped = Vec::new();
    let mut unreadable = Vec::new();
    for path in &files {
        let xml = std::fs::read_to_string(path).unwrap();
        let label = path.strip_prefix(&root).unwrap().display().to_string();
        let parsed = match parse_srgfx(&xml) {
            Ok(parsed) => parsed,
            Err(error) => {
                unreadable.push(format!("{label}: {error:#}"));
                continue;
            }
        };
        for snapshot in parsed {
            if param_layout(snapshot.fx_type).is_none() {
                skipped.push(format!("{label} (type {})", snapshot.fx_type));
                continue;
            }
            let ranges = match ranges_by_type.get(&snapshot.fx_type) {
                Some(ranges) => ranges.clone(),
                None => {
                    let ranges = renderer.measure_surge_fx_ranges(snapshot.fx_type).unwrap();
                    ranges_by_type.insert(snapshot.fx_type, ranges.clone());
                    ranges
                }
            };
            load_and_check(
                &mut renderer,
                &snapshot,
                &ranges,
                &format!("{label} #{}", snapshot.name),
            );
            snapshots += 1;
        }
    }
    eprintln!(
        "files={} snapshots={snapshots} types={} skipped={skipped:?} unreadable={unreadable:?}",
        files.len(),
        ranges_by_type.len()
    );
    assert!(snapshots > 0);
    assert!(
        unreadable.len() <= 1,
        "読めない .srgfx が増えている: {unreadable:?}"
    );
}

fn collect_srgfx(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_srgfx(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("srgfx") {
            out.push(path);
        }
    }
}
