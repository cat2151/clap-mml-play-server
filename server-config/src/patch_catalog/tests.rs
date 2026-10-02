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
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sforzando_ignores_configured_dirs() {
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "cmrt_patch_catalog_sforzando_configured_{}_{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Programs")).unwrap();
    std::fs::write(root.join("Programs/Piano.sfz"), b"<region>").unwrap();
    std::fs::write(
        root.join("Bank.bank.xml"),
        br#"<AriaBank id="1" version="1"><AriaProgram name="Piano"><AriaElement path="Programs/Piano.sfz"/></AriaProgram></AriaBank>"#,
    )
    .unwrap();
    let configured = vec![
        root.to_string_lossy().into_owned(),
        root.join("not-there").to_string_lossy().into_owned(),
    ];

    for (with_toml, without_toml) in [
        (
            resolve_patch_catalog_roots(
                Some(crate::SFORZANDO_PLUGIN_ID),
                "missing-sforzando-a.clap",
                Some(&configured),
            ),
            resolve_patch_catalog_roots(
                Some(crate::SFORZANDO_PLUGIN_ID),
                "missing-sforzando-a.clap",
                None,
            ),
        ),
        (
            resolve_patch_catalog(
                Some(crate::SFORZANDO_PLUGIN_ID),
                "missing-sforzando-b.clap",
                Some(&configured),
            ),
            sforzando::resolve_catalog(sforzando::RegistrySources::read(false)).into(),
        ),
    ] {
        assert!(with_toml.dirs.is_empty(), "{:?}", with_toml.dirs);
        assert!(with_toml.configured_missing.is_empty());
        assert_eq!(with_toml, without_toml);
    }
    let _ = std::fs::remove_dir_all(root);
}

/// configured の `patches_dirs` を渡しても、実機の registry から引く root は変わらない。
#[test]
#[ignore = "実機の sforzando と ARIA の registry が要る"]
fn installed_sforzando_roots_ignore_configured_dirs() {
    let plugin = std::env::var("CMRT_TEST_SFORZANDO_CLAP").unwrap();
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let missing = std::env::temp_dir().join(format!(
        "cmrt_patch_catalog_sforzando_installed_{}_{id}",
        std::process::id()
    ));
    let configured = vec![missing.to_string_lossy().into_owned()];

    let plain = resolve_patch_catalog_roots(Some(crate::SFORZANDO_PLUGIN_ID), &plugin, None);
    let with_toml =
        resolve_patch_catalog_roots(Some(crate::SFORZANDO_PLUGIN_ID), &plugin, Some(&configured));
    let catalog = resolve_patch_catalog(Some(crate::SFORZANDO_PLUGIN_ID), &plugin, None);

    assert!(!plain.dirs.is_empty());
    assert_eq!(plain, with_toml);
    assert_eq!(plain.dirs, catalog.dirs);
}

/// 全 program の display が置き場のフォルダ名で始まり、解決すると元のファイルへ戻る。
#[test]
#[ignore = "実機の sforzando と ARIA の registry が要る"]
fn installed_sforzando_displays_round_trip_through_per_root_base() {
    let plugin = std::env::var("CMRT_TEST_SFORZANDO_CLAP").unwrap();

    let resolution = resolve_patch_catalog(Some(crate::SFORZANDO_PLUGIN_ID), &plugin, None);

    let root_names = resolution
        .dirs
        .iter()
        .map(|dir| {
            Path::new(dir)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    let patches = resolution.resolved_patches.unwrap();
    assert!(!patches.is_empty());
    let mut per_root = std::collections::BTreeMap::<String, usize>::new();
    for path in &patches {
        let display = resolution.base.display(path);
        let first = display.split('/').next().unwrap().to_string();
        assert!(root_names.contains(&first), "{display} ({root_names:?})");
        assert_eq!(
            resolution.base.resolve(&display),
            path.to_string_lossy(),
            "{display}"
        );
        *per_root.entry(first).or_default() += 1;
    }
    eprintln!("per-root display counts: {per_root:?}");
}
