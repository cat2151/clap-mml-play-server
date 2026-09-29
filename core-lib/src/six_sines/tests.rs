use super::*;

#[test]
fn recognizes_six_sines_patches_case_insensitively() {
    assert!(is_six_sines_patch_path("Bass/Bass 1.sxsnp"));
    assert!(is_six_sines_patch_path("Bass\\Bass 1.SXSNP"));
    assert!(!is_six_sines_patch_path(".sxsnp"));
    assert!(!is_six_sines_patch_path("Bass/Bass 1.fxp"));
}

fn patch_with_play_mode(value: &str) -> String {
    format!(
        r#"<patch id="org.baconpaul.six-sines" version="6" name="X"><params><p id="509" v="0.000000" /><p id="1523" v="1.000000" /><p id="523" v="{value}" /><p id="500" v="0.750000" /></params></patch>"#
    )
}

#[test]
fn play_mode_one_is_mono_and_zero_is_poly() {
    assert_eq!(
        six_sines_voicing(&patch_with_play_mode("1.000000")),
        PatchVoicing::Mono
    );
    assert_eq!(
        six_sines_voicing(&patch_with_play_mode("0.000000")),
        PatchVoicing::Poly
    );
}

/// 別の id の末尾が `523` でも拾わない。param の並びが id 順でなくても見つける。
#[test]
fn only_the_exact_param_id_is_read() {
    let xml = r#"<patch id="org.baconpaul.six-sines"><params><p id="1523" v="1.000000" /><p id="523" v="0.000000" /></params></patch>"#;
    assert_eq!(six_sines_voicing(xml), PatchVoicing::Poly);
}

/// 読めないときは黙って Poly にしない。
#[test]
fn a_missing_or_broken_play_mode_is_unknown() {
    let without =
        r#"<patch id="org.baconpaul.six-sines"><params><p id="500" v="0.75" /></params></patch>"#;
    assert_eq!(six_sines_voicing(without), PatchVoicing::Unknown);
    assert_eq!(
        six_sines_voicing(&patch_with_play_mode("mono")),
        PatchVoicing::Unknown
    );
    assert_eq!(six_sines_voicing(""), PatchVoicing::Unknown);
    assert_eq!(
        read_six_sines_voicing(Path::new("does-not-exist.sxsnp")),
        PatchVoicing::Unknown
    );
}
