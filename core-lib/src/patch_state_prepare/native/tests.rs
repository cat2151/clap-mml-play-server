use super::*;

fn absolute(relative: &str) -> std::path::PathBuf {
    Path::new(if cfg!(windows) {
        "X:/"
    } else {
        "/missing-cmrt-native-contract"
    })
    .join(relative)
}

#[test]
fn rejects_identity_and_format_before_loading_native_code() {
    let bundle = absolute("plugins/missing.clap");
    let patch = absolute("patches/missing.floe-preset");
    let (bundle, patch) = (bundle.as_path(), patch.as_path());
    assert!(matches!(
        prepare_native_clap_patch_state("unknown", bundle, patch),
        Err(PatchStateError::UnsupportedPlugin { .. })
    ));
    assert!(matches!(
        prepare_native_clap_patch_state(FLOE_PLUGIN_ID, bundle, &absolute("patches/other.sfz")),
        Err(PatchStateError::UnsupportedFormat { .. })
    ));
    assert!(matches!(
        prepare_native_clap_patch_state(SFORZANDO_PLUGIN_ID, bundle, patch),
        Err(PatchStateError::UnsupportedFormat { .. })
    ));
    assert!(matches!(
        prepare_native_clap_patch_state(FLOE_PLUGIN_ID, bundle, Path::new("relative.floe-preset")),
        Err(PatchStateError::InvalidPath { .. })
    ));
}

#[test]
fn missing_native_patch_is_an_io_error() {
    assert!(matches!(
        prepare_native_clap_patch_state(
            FLOE_PLUGIN_ID,
            &absolute("plugins/missing.clap"),
            &absolute("patches/missing.floe-preset")
        ),
        Err(PatchStateError::Io { .. })
    ));
}
