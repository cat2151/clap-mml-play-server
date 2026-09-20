//! TONE3000（`com.tone3000.plugin`）を `EffectRenderer` で host する実測。

use std::path::PathBuf;

use super::*;
use crate::juce_value_tree::{encode, ValueTree, Var};
use crate::tone3000_preset::{parse_state, parse_t3k_preset, Tone3000Preset, TONE3000_PLUGIN_ID};

const TONE3000_CLAP_ENV: &str = "CMRT_TEST_TONE3000_CLAP";
const TONE3000_PRESETS_ENV: &str = "CMRT_TEST_TONE3000_PRESETS";

fn tone3000_renderer() -> EffectRenderer {
    let entry = load_entry(&plugin_path(TONE3000_CLAP_ENV)).unwrap();
    EffectRenderer::new(&entry, TONE3000_PLUGIN_ID, SAMPLE_RATE, BUFFER_SIZE).unwrap()
}

/// preset ディレクトリの `.t3kpreset` を (uuid, preset) で名前順に返す。
fn factory_presets() -> Vec<(String, Tone3000Preset)> {
    let dir = PathBuf::from(plugin_path(TONE3000_PRESETS_ENV));
    let mut presets: Vec<(String, Tone3000Preset)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("t3kpreset"))
        .map(|path| {
            let uuid = path.file_stem().unwrap().to_string_lossy().into_owned();
            let preset = parse_t3k_preset(&std::fs::read(&path).unwrap())
                .unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
            (uuid, preset)
        })
        .collect();
    assert!(
        !presets.is_empty(),
        "{} に .t3kpreset が無い",
        dir.display()
    );
    presets.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    presets
}

fn chain_block_types(state: &ValueTree) -> Vec<String> {
    let mut types = Vec::new();
    fn walk(tree: &ValueTree, types: &mut Vec<String>) {
        if tree.type_name == "ChainBlock" {
            if let Some(kind) = tree.property_string("type") {
                types.push(kind.to_string());
            }
        }
        for child in &tree.children {
            walk(child, types);
        }
    }
    if let Some(chain) = state.child("ChainSnapshot") {
        walk(chain, &mut types);
    }
    types
}

fn saved_tree(renderer: &mut EffectRenderer) -> ValueTree {
    parse_state(&renderer.save_state().unwrap()).unwrap()
}

fn sine_220hz(seconds: usize) -> Vec<f32> {
    let frames = (SAMPLE_RATE as usize * seconds).next_multiple_of(BUFFER_SIZE);
    (0..frames)
        .flat_map(|index| {
            let sample =
                0.25 * (2.0 * std::f64::consts::PI * 220.0 * index as f64 / SAMPLE_RATE).sin();
            [sample as f32, sample as f32]
        })
        .collect()
}

#[test]
#[ignore = "実 TONE3000 CLAP が要る"]
fn tone3000_init_state_is_a_value_tree_that_round_trips() {
    let started = std::time::Instant::now();
    let mut renderer = tone3000_renderer();
    let instantiate = started.elapsed();
    let init = renderer.init_state().to_vec();
    let tree = parse_state(&init).unwrap();
    let mut re_encoded = b"T3KB".to_vec();
    re_encoded.extend(encode(&tree));
    assert_eq!(re_encoded, init, "init state の往復が一致しない");
    assert_eq!(renderer.input_port_channels(), vec![2]);
    let started = std::time::Instant::now();
    renderer.ensure_active().unwrap();
    let activate = started.elapsed();
    let latency = renderer.latency();
    let params = renderer.param_names().len();
    eprintln!(
        "instantiate={instantiate:?} activate+start={activate:?} latency={latency} params={params}"
    );
}

/// `activePresetId` / `activePresetName` を書くだけでは preset は載らない。
#[test]
#[ignore = "実 TONE3000 CLAP と factory preset が要る"]
fn tone3000_naming_the_preset_alone_does_not_load_it() {
    let mut renderer = tone3000_renderer();
    let (uuid, preset) = factory_presets().into_iter().next().unwrap();
    let mut tree = parse_state(renderer.init_state()).unwrap();
    tree.set_property("activePresetId", Var::String(uuid.clone()));
    tree.set_property("activePresetName", Var::String(preset.name.clone()));
    let mut blob = b"T3KB".to_vec();
    blob.extend(encode(&tree));
    renderer.load_state(&blob).unwrap();
    let blocks = chain_block_types(&saved_tree(&mut renderer));
    eprintln!("after name-only load: blocks={blocks:?}");
    assert!(
        !blocks.iter().any(|kind| kind == "nam"),
        "名前だけで preset が載った: {blocks:?}"
    );
}

/// `load_tone3000_preset` で preset が載り（自己申告の名前と NAM block）、音が変わる。
#[test]
#[ignore = "実 TONE3000 CLAP と factory preset が要る"]
fn tone3000_preset_loads_and_changes_the_sound() {
    let mut renderer = tone3000_renderer();
    let (uuid, preset) = factory_presets()
        .into_iter()
        .find(|(_, preset)| preset.name == "Bogner Fullstack")
        .expect("Bogner Fullstack が factory preset に無い");

    let started = std::time::Instant::now();
    let waited = renderer.load_tone3000_preset(&preset, &uuid).unwrap();
    let load_total = started.elapsed();
    let saved = saved_tree(&mut renderer);
    let blocks = chain_block_types(&saved);
    eprintln!(
        "load_tone3000_preset={load_total:?} (audible after {waited:?}) activePresetName={:?} blocks={blocks:?} latency={}",
        saved.property_string("activePresetName"),
        renderer.latency()
    );
    assert_eq!(
        saved.property_string("activePresetName"),
        Some(preset.name.as_str())
    );
    assert!(
        blocks.iter().any(|kind| kind == "nam"),
        "NAM block が無い: {blocks:?}"
    );

    let mut chain = EffectChain::new(vec![renderer]).unwrap();
    let input = sine_220hz(1);
    let mut wet = input.clone();
    chain
        .process_all(&mut wet, EffectTransport::default())
        .unwrap();
    assert_eq!(wet.len(), input.len());
    let frames = input.len() / 2;
    let (start, end) = (frames / 4, frames);
    let input_rms = rms_dbfs_frames(&input, start, end);
    let wet_rms = rms_dbfs_frames(&wet, start, end);
    let diff: Vec<f32> = wet.iter().zip(&input).map(|(a, b)| a - b).collect();
    let diff_rms = rms_dbfs_frames(&diff, start, end);
    eprintln!(
        "220 Hz sine: input={input_rms:.1} dBFS wet={wet_rms:.1} dBFS wet-input={diff_rms:.1} dBFS"
    );
    assert!(wet_rms > -60.0, "TONE3000 の出力が無音: {wet_rms} dBFS");
    assert!(
        diff_rms > input_rms - 20.0,
        "出力が入力とほぼ同じ（effect が効いていない）: diff {diff_rms} dBFS"
    );
}

/// factory preset 7 件すべてが、preset ごとに新規の instance へ載って音が出る。
#[test]
#[ignore = "実 TONE3000 CLAP と factory preset が要る"]
fn tone3000_every_factory_preset_loads_by_name() {
    let presets = factory_presets();
    for (uuid, preset) in &presets {
        let mut renderer = tone3000_renderer();
        let started = std::time::Instant::now();
        let waited = renderer.load_tone3000_preset(preset, uuid).unwrap();
        let load_total = started.elapsed();
        let saved = saved_tree(&mut renderer);
        let blocks = chain_block_types(&saved);
        eprintln!(
            "{uuid} '{}' load={load_total:?} audible after {waited:?} blocks={blocks:?}",
            preset.name
        );
        assert_eq!(
            saved.property_string("activePresetName"),
            Some(preset.name.as_str())
        );
        assert!(
            blocks.iter().any(|kind| kind == "nam"),
            "{}: {blocks:?}",
            preset.name
        );
    }
    let names: Vec<&str> = presets
        .iter()
        .map(|(_, preset)| preset.name.as_str())
        .collect();
    eprintln!("presets: {names:?}");
    assert_eq!(
        presets.len(),
        names.iter().collect::<std::collections::HashSet<_>>().len(),
        "同名の preset がある"
    );
}
