//! Voyage Voyage（`com.MusicalEntropy.VoyageVoyagex1`）を `EffectRenderer` で host する実測。

use std::path::{Path, PathBuf};

use super::*;
use crate::audio_effect::PresetLocation;
use crate::surge_fx_preset::juce_xml;
use crate::voyage_voyage_preset::{parse_voyage_xml, PST_EXTENSION, VOYAGE_VOYAGE_PLUGIN_ID};

const VOYAGE_CLAP_ENV: &str = "CMRT_TEST_VOYAGE_CLAP";
const VOYAGE_PRESETS_ENV: &str = "CMRT_TEST_VOYAGE_PRESETS";

/// param の index 順に並べた、state の属性名。
const PARAM_ATTRIBUTES: [&str; 18] = [
    "INPUT", "OUTPUT", "FEED", "DECAY", "TONE", "MIX", "EXCIT", "CHAOS", "SHIMAMT", "SHIMDLY",
    "SHIMP", "LC", "HC", "ENGINE", "DRONEF", "DRONES", "TRUEST", "BYPASS",
];
/// state の `ENGINE` は 0..3 の整数、param は 0..1。
const ENGINE_STEPS: f64 = 3.0;

fn renderer() -> EffectRenderer {
    let entry = load_entry(&plugin_path(VOYAGE_CLAP_ENV)).unwrap();
    EffectRenderer::new(&entry, VOYAGE_VOYAGE_PLUGIN_ID, SAMPLE_RATE, BUFFER_SIZE).unwrap()
}

fn collect_pst(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_pst(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some(PST_EXTENSION) {
            out.push(path);
        }
    }
}

fn number(text: &str, label: &str) -> f64 {
    text.trim()
        .parse()
        .unwrap_or_else(|error| panic!("{label}: '{text}': {error}"))
}

#[test]
#[ignore = "実 Voyage Voyage CLAP と preset が要る"]
fn voyage_every_pst_round_trips_through_the_plugin_state() {
    let root = PathBuf::from(plugin_path(VOYAGE_PRESETS_ENV));
    let mut files = Vec::new();
    collect_pst(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "{} に .pst が無い", root.display());
    let mut renderer = renderer();
    let init = parse_voyage_xml(&juce_xml::decode(renderer.init_state()).unwrap()).unwrap();
    for path in &files {
        let label = path.strip_prefix(&root).unwrap().display().to_string();
        let preset = parse_voyage_xml(&std::fs::read_to_string(path).unwrap()).unwrap();
        renderer
            .load_preset(&PresetLocation {
                path: path.clone(),
                value: label.clone(),
                display: format!("Voyage Voyage: {label}"),
            })
            .unwrap_or_else(|error| panic!("{label}: {error:#}"));
        let saved =
            parse_voyage_xml(&juce_xml::decode(&renderer.save_state().unwrap()).unwrap()).unwrap();
        for (name, expected) in &init.attributes {
            let expected = preset.attributes.get(name).unwrap_or(expected);
            let actual = saved
                .attributes
                .get(name)
                .unwrap_or_else(|| panic!("{label}: 保存し直した state に {name} が無い"));
            let (expected, actual) = (number(expected, &label), number(actual, &label));
            assert!(
                (expected - actual).abs() < 1e-6,
                "{label}: {name} は {expected} のはずが {actual}"
            );
        }
        let params = renderer.param_values();
        assert_eq!(params.len(), PARAM_ATTRIBUTES.len(), "{label}: {params:?}");
        for ((param, value), name) in params.iter().zip(PARAM_ATTRIBUTES) {
            let mut expected = number(&saved.attributes[name], &label);
            if name == "ENGINE" {
                expected /= ENGINE_STEPS;
            }
            assert!(
                (expected - value).abs() < 1e-6,
                "{label}: param {param}（{name}）は {expected} のはずが {value}"
            );
        }
    }
    println!("Voyage Voyage: {} 件の .pst を load した", files.len());
}
