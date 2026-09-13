use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

static TEMP_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn generic_facade_does_not_apply_vendor_resolution_to_other_plugins() {
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "cmrt_patch_catalog_plain_{}_{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let configured = vec![root.to_string_lossy().into_owned()];

    let resolution = resolve_patch_catalog(
        Some(crate::SURGE_XT_PLUGIN_ID),
        "Surge XT.clap",
        Some(&configured),
    );

    assert_eq!(resolution.resolved_patches, None);
    assert_eq!(resolution.dirs.len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn root_facade_does_not_enumerate_sforzando_programs() {
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "cmrt_patch_catalog_sforzando_roots_{}_{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(root.join("nested/Piano.sfz"), b"<region>").unwrap();
    let configured = vec![root.to_string_lossy().into_owned()];

    let resolution = resolve_patch_catalog_roots(
        Some(crate::SFORZANDO_PLUGIN_ID),
        "missing-sforzando.clap",
        Some(&configured),
    );

    assert_eq!(resolution.resolved_patches, None);
    assert_eq!(resolution.dirs.len(), 1);
    assert!(resolution.source_error.is_none());
    let _ = std::fs::remove_dir_all(root);
}
