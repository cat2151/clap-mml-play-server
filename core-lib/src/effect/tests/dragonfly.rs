//! Dragonfly Reverb（`michaelwillis.dragonfly.*`）を `EffectRenderer` で host する実測。

use std::path::{Path, PathBuf};

use super::*;
use crate::audio_effect::PresetLocation;
use crate::dragonfly_preset::{parse_state, DragonflyPlugin, DRAGONFLY_PLUGINS};

const DRAGONFLY_DIR_ENV: &str = "CMRT_TEST_DRAGONFLY_DIR";

fn clap_path(plugin: &DragonflyPlugin) -> PathBuf {
    Path::new(&plugin_path(DRAGONFLY_DIR_ENV)).join(format!("{}.clap", plugin.file_stem))
}

fn renderer(plugin: &DragonflyPlugin) -> EffectRenderer {
    let entry = load_entry(&clap_path(plugin).to_string_lossy()).unwrap();
    EffectRenderer::new(&entry, plugin.plugin_id, SAMPLE_RATE, BUFFER_SIZE).unwrap()
}

fn location(plugin: &DragonflyPlugin, preset: &str) -> PresetLocation {
    PresetLocation {
        path: clap_path(plugin),
        value: preset.to_string(),
        display: format!("{}: {preset}", plugin.name),
    }
}

/// 保存し直した state の `key → value`（state 欄と parameter 欄の区別はしない）。
fn saved_values(renderer: &mut EffectRenderer) -> Vec<(String, String)> {
    let tokens = parse_state(&renderer.save_state().unwrap()).unwrap();
    tokens
        .iter()
        .filter(|token| !token.starts_with("__dpf_"))
        .cloned()
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect()
}

#[test]
#[ignore = "実 Dragonfly Reverb CLAP が要る"]
fn dragonfly_every_builtin_preset_matches_the_plugin_self_report() {
    for plugin in &DRAGONFLY_PLUGINS {
        for preset in plugin.presets {
            let mut renderer = renderer(plugin);
            renderer
                .load_preset(&location(plugin, preset.name))
                .unwrap_or_else(|error| panic!("{}: {}: {error:#}", plugin.name, preset.name));
            let saved = saved_values(&mut renderer);
            let lookup = |key: &str| {
                saved
                    .iter()
                    .find(|(saved_key, _)| saved_key == key)
                    .map(|(_, value)| value.clone())
                    .unwrap_or_else(|| {
                        panic!("{}: state に '{key}' が無い: {saved:?}", plugin.name)
                    })
            };
            if plugin.has_preset_state {
                assert_eq!(lookup("preset"), preset.name, "{}", plugin.name);
            }
            for (symbol, expected) in plugin.symbols.iter().zip(preset.values) {
                let actual: f32 = lookup(symbol).parse().unwrap();
                assert!(
                    (actual - expected).abs() <= expected.abs() * 1e-4 + 1e-4,
                    "{}: {}: {symbol} = {actual}（表は {expected}）",
                    plugin.name,
                    preset.name
                );
            }
        }
        eprintln!("{}: {} presets OK", plugin.name, plugin.presets.len());
    }
}

/// impulse を通し、1〜2 s の区間の残響の RMS を返す。
fn tail_dbfs(plugin: &DragonflyPlugin, preset: &str) -> f32 {
    let mut renderer = renderer(plugin);
    renderer.load_preset(&location(plugin, preset)).unwrap();
    let mut chain = EffectChain::new(vec![renderer]).unwrap();
    let frames = (SAMPLE_RATE as usize * 3).next_multiple_of(BUFFER_SIZE);
    let mut samples = vec![0.0_f32; frames * 2];
    samples[0] = 1.0;
    samples[1] = 1.0;
    chain
        .process_all(&mut samples, EffectTransport::default())
        .unwrap();
    let second = SAMPLE_RATE as usize;
    rms_dbfs_frames(&samples, second, 2 * second)
}

#[test]
#[ignore = "実 Dragonfly Reverb CLAP が要る"]
fn dragonfly_long_preset_rings_longer_than_short_preset() {
    let hall = &DRAGONFLY_PLUGINS[0];
    let great = tail_dbfs(hall, "Great Hall");
    let bright = tail_dbfs(hall, "Bright Room");
    eprintln!("tail 1-2 s: Great Hall {great:.1} dBFS, Bright Room {bright:.1} dBFS");
    assert!(great > -80.0, "Great Hall の残響が無い: {great}");
    assert!(
        great > bright + 20.0,
        "decay 3.8 s の Great Hall が decay 0.6 s の Bright Room より長く鳴らない: {great} / {bright}"
    );
}
