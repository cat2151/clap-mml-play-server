//! Shu（`audio.mikey.Shu`）を `EffectRenderer` で host する実測。

use super::*;
use crate::audio_effect::PresetLocation;
use crate::shu_preset::{SHU_PLUGIN_ID, SHU_PRESETS};

const SHU_CLAP_ENV: &str = "CMRT_TEST_SHU_CLAP";

fn renderer() -> EffectRenderer {
    let entry = load_entry(&plugin_path(SHU_CLAP_ENV)).unwrap();
    EffectRenderer::new(&entry, SHU_PLUGIN_ID, SAMPLE_RATE, BUFFER_SIZE).unwrap()
}

fn location(preset: &str) -> PresetLocation {
    PresetLocation {
        path: plugin_path(SHU_CLAP_ENV).into(),
        value: preset.to_string(),
        display: format!("Shu: {preset}"),
    }
}

fn value_of(values: &[(String, f64)], name: &str) -> f64 {
    values
        .iter()
        .find(|(param, _)| param == name)
        .map(|(_, value)| *value)
        .unwrap_or_else(|| panic!("Shu に param '{name}' が無い: {values:?}"))
}

#[test]
#[ignore = "実 Shu CLAP が要る"]
fn shu_every_preset_reads_back_its_algorithm_and_decay() {
    let init = renderer().param_values();
    assert_eq!(value_of(&init, "Decay"), 0.5);
    for preset in &SHU_PRESETS {
        let mut renderer = renderer();
        renderer
            .load_preset(&location(preset.name))
            .unwrap_or_else(|error| panic!("{}: {error:#}", preset.name));
        let loaded = renderer.param_values();
        assert_eq!(
            value_of(&loaded, "Algorithm"),
            f64::from(preset.algorithm),
            "{}",
            preset.name
        );
        let decay = if preset.decay_max { 1.0 } else { 0.5 };
        assert_eq!(value_of(&loaded, "Decay"), decay, "{}", preset.name);
        for name in ["Mix", "Ether Rain", "Ether Shine"] {
            assert_eq!(
                value_of(&loaded, name),
                value_of(&init, name),
                "{}: {name}",
                preset.name
            );
        }
        let changed: Vec<&str> = init
            .iter()
            .zip(&loaded)
            .filter(|((_, before), (_, after))| before != after)
            .map(|((name, _), _)| name.as_str())
            .collect();
        assert!(
            changed
                .iter()
                .all(|name| ["Algorithm", "Decay"].contains(name)),
            "{}: Algorithm / Decay 以外が init から変わった: {changed:?}",
            preset.name
        );
    }
}
