use super::*;
use std::path::Path;

fn test_bases() -> PatchBases {
    PatchBases::from_bases(Some("/patches"), Some("/cartridges"))
}

#[test]
fn resolve_live_patch_uses_root_only_for_relative_paths() {
    let relative = resolve_live_patch(Some("Keys/Piano.fxp".into()), &test_bases()).unwrap();
    assert_eq!(
        relative,
        Path::new("/patches")
            .join("Keys/Piano.fxp")
            .to_string_lossy()
    );
    assert_eq!(resolve_live_patch(Some("  ".into()), &test_bases()), None);
}

/// 音色置き場はプラグインごとに別の場所なので、cartridge の相対パスを Surge の
/// 音色置き場へ join してしまうと存在しないファイルを指す。
#[test]
fn resolve_live_patch_picks_the_base_that_matches_the_patch_form() {
    let cartridge =
        resolve_live_patch(Some("Dexed_01.syx/00 Say Again.".into()), &test_bases()).unwrap();

    assert_eq!(
        cartridge,
        Path::new("/cartridges")
            .join("Dexed_01.syx/00 Say Again.")
            .to_string_lossy()
    );
}

#[test]
fn configured_live_instance_count_rejects_higher_ids() {
    assert!(validate_live_instance_id(3, 4).is_ok());
    let error = validate_live_instance_id(4, 4).unwrap_err();
    assert!(error.to_string().contains("configured live range 0..4"));
}

#[test]
fn live_instance_state_matches_configured_count() {
    assert_eq!(runtime::new_live_instances(4).len(), 4);
}
