use super::*;
use std::path::Path;

#[test]
fn resolve_live_patch_uses_root_only_for_relative_paths() {
    let relative = resolve_live_patch(Some("Keys/Piano.fxp".into()), Some("/patches")).unwrap();
    assert_eq!(
        relative,
        Path::new("/patches")
            .join("Keys/Piano.fxp")
            .to_string_lossy()
    );
    assert_eq!(
        resolve_live_patch(Some("  ".into()), Some("/patches")),
        None
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
