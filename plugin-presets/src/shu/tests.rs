use super::*;

/// slot i に `100 + i` を入れた state。どの slot が変わったかを値で見分けられる。
fn synthetic_init_state() -> Vec<u8> {
    (0..SHU_PARAM_COUNT)
        .flat_map(|slot| (100.0 + slot as f64).to_le_bytes())
        .collect()
}

fn slots(state: &[u8]) -> Vec<f64> {
    state
        .as_chunks::<SLOT_BYTES>()
        .0
        .iter()
        .map(|bytes| f64::from_le_bytes(*bytes))
        .collect()
}

#[test]
fn each_preset_changes_only_its_own_slots() {
    let init = synthetic_init_state();
    let before = slots(&init);
    for preset in &SHU_PRESETS {
        let after = slots(&shu_state_blob(&init, preset).unwrap());
        let changed: Vec<usize> = (0..SHU_PARAM_COUNT)
            .filter(|slot| before[*slot] != after[*slot])
            .collect();
        let expected = if preset.name == "Ether Decay Max" {
            vec![SHU_DECAY_SLOT, SHU_ALGORITHM_SLOT]
        } else {
            vec![SHU_ALGORITHM_SLOT]
        };
        assert_eq!(changed, expected, "{}", preset.name);
        assert_eq!(
            after[SHU_ALGORITHM_SLOT],
            f64::from(preset.algorithm),
            "{}",
            preset.name
        );
        if preset.decay_max {
            assert_eq!(after[SHU_DECAY_SLOT], DECAY_MAX_SLOT_VALUE);
        }
    }
}

#[test]
fn presets_cover_every_algorithm_by_name() {
    let names: Vec<(&str, u8)> = SHU_PRESETS
        .iter()
        .map(|preset| (preset.name, preset.algorithm))
        .collect();
    assert_eq!(
        names,
        [
            ("Silk", 0),
            ("Drift", 1),
            ("Plate", 2),
            ("Classic", 3),
            ("Slap", 4),
            ("Spring", 5),
            ("Ether", 6),
            ("Ether Decay Max", 6),
        ]
    );
}

#[test]
fn ether_presets_are_shimmer_reverb_and_the_rest_are_reverb() {
    for value in shu_preset_values() {
        let expected = if value.value.starts_with("Ether") {
            "Shimmer Reverb"
        } else {
            "Reverb"
        };
        assert_eq!(value.kind, expected, "{}", value.value);
        assert_eq!(value.category, "Space / Imaging");
        assert_eq!(value.shown, value.value);
    }
}

#[test]
fn wrong_state_length_is_an_error() {
    let error = shu_state_blob(&[0; 279], &SHU_PRESETS[0]).unwrap_err();
    assert!(error.to_string().contains("279"), "{error}");
}

#[test]
fn unknown_preset_name_is_an_error() {
    assert!(shu_preset("Hall").is_err());
    assert_eq!(shu_preset("Ether").unwrap().algorithm, 6);
}
