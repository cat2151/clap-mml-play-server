use super::*;

#[test]
fn wav_paths_are_recognized_regardless_of_case() {
    assert!(is_cache_wav_patch_path("track2_meas1.wav"));
    assert!(is_cache_wav_patch_path(r"C:\cache\Track2_Meas1.WAV"));
    assert!(is_cache_wav_patch_path("  track2_meas1.wav  "));
}

#[test]
fn other_patch_forms_are_not_cache_wav() {
    for patch in [
        "Keys/Piano.fxp",
        "Dexed_01.syx/00 Say Again.",
        "AR Accent Arp.vvp",
        "Harp/Realistic.floe-preset",
        "Garritan/Glockenspiel.sfz",
    ] {
        assert!(!is_cache_wav_patch_path(patch), "{patch}");
    }
}

#[test]
fn state_is_the_path_itself_not_the_file_contents() {
    let dir = std::env::temp_dir().join("cmrt-cache-wav-state-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("track1_meas1.wav");
    std::fs::write(&path, b"not really a wav").unwrap();

    let state = cache_wav_state(path.to_str().unwrap()).unwrap();
    assert_eq!(state, path.to_str().unwrap().as_bytes());
}

#[test]
fn missing_file_reports_the_path() {
    let error = cache_wav_state("N:/does/not/exist.wav").unwrap_err();
    assert!(error.to_string().contains("exist.wav"), "{error}");
}

/// スロット指定つきの綴りも同じ patch 文字列として routing されること。
#[test]
fn a_slot_prefixed_path_is_still_a_cache_wav_patch() {
    assert!(is_cache_wav_patch_path(&cache_wav_patch_with_slot(
        1,
        r"C:\cache\track2_meas3.wav"
    )));
    assert!(is_cache_wav_patch_path("slot=0;track2_meas1.wav"));
    assert!(is_cache_wav_patch_path("  slot=1;track2_meas2.wav  "));
}

/// **綴りを変えても拡張子判定が壊れないこと。** サフィックス形にするとここが壊れる。
///
/// `resolve_patch_target` は拡張子でプラグインを選ぶので、これが崩れると
/// キャッシュ WAV が Surge XT の state file として扱われる。
#[test]
fn the_slot_spelling_keeps_the_wav_extension_of_the_whole_patch_string() {
    let patch = cache_wav_patch_with_slot(1, r"C:\cache\track2_meas3.wav");

    assert_eq!(
        Path::new(&patch).extension().and_then(|ext| ext.to_str()),
        Some("wav"),
        "{patch}"
    );
}

/// 壊れたスロット番号は「cache-player 宛ての誤り」として断ること。
///
/// ここで拾わないと Surge XT の state file へ流れ、原因の遠いエラーになる。
#[test]
fn a_broken_slot_number_is_rejected_here_not_routed_to_another_plugin() {
    for patch in ["slot=9;C:/x.wav", "slot=x;C:/x.wav", "slot=1"] {
        assert!(is_cache_wav_patch_path(patch), "{patch}");
        assert!(cache_wav_state(patch).is_err(), "{patch}");
    }
}

/// スロットを空にする綴りは、読むファイルが無くても通ること。
#[test]
fn clearing_a_slot_needs_no_file_on_disk() {
    assert!(is_cache_wav_patch_path("slot=1;"));
    assert_eq!(cache_wav_state("slot=1;").unwrap(), b"slot=1;".to_vec());
}

/// スロット指定つきでも、実在しないパスはパス付きで断ること。
#[test]
fn a_missing_file_reports_the_path_even_with_a_slot_prefix() {
    let error = cache_wav_state("slot=1;N:/does/not/exist.wav").unwrap_err();

    assert!(error.to_string().contains("exist.wav"), "{error}");
    assert!(!error.to_string().contains("slot="), "{error}");
}

/// state は綴りそのまま（`slot=0;` へ書き換えたりしない）。
#[test]
fn the_state_is_the_patch_string_itself() {
    let dir = std::env::temp_dir().join("cmrt-cache-wav-state-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("track1_meas2.wav");
    std::fs::write(&path, b"not really a wav").unwrap();
    let patch = cache_wav_patch_with_slot(1, path.to_str().unwrap());

    let state = cache_wav_state(&patch).unwrap();

    assert_eq!(state, patch.as_bytes());
}
