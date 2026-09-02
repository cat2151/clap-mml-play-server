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
