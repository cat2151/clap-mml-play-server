use super::*;

#[test]
fn recognizes_floe_presets_case_insensitively() {
    assert!(is_floe_preset_path("Bank/Piano.floe-preset"));
    assert!(is_floe_preset_path("Bank\\Piano.FLOE-PRESET"));
    assert!(!is_floe_preset_path(".floe-preset"));
    assert!(!is_floe_preset_path("Library.floe-pkg"));
}
