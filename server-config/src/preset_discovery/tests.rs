use super::*;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "cmrt_sforzando_discovery_{name}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn discovery_and_config_are_unioned_and_canonical_duplicates_are_removed() {
    let root = temp_root("union");
    let configured = root.join("configured");
    let discovered = root.join("discovered");
    std::fs::create_dir_all(&configured).unwrap();
    std::fs::create_dir_all(&discovered).unwrap();
    let configured_text = configured.to_string_lossy().into_owned();
    let configured_dirs = vec![configured_text.clone()];

    let resolution = resolve_patch_dirs(
        Some(&configured_dirs),
        Ok(vec![configured.join("."), discovered.clone()]),
    );

    assert_eq!(resolution.dirs.len(), 2);
    assert!(resolution.dirs.contains(
        &std::fs::canonicalize(configured)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    ));
    assert!(resolution.dirs.contains(
        &std::fs::canonicalize(discovered)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    ));
    assert!(resolution.discovery_error.is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn valid_config_survives_a_discovery_failure() {
    let root = temp_root("fallback");
    let configured_text = root.to_string_lossy().into_owned();
    let configured_dirs = vec![configured_text];

    let resolution = resolve_patch_dirs(
        Some(&configured_dirs),
        Err(anyhow::anyhow!("provider init failed")),
    );

    assert_eq!(resolution.dirs.len(), 1);
    assert!(resolution
        .discovery_error
        .as_deref()
        .is_some_and(|error| error.contains("provider init failed")));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn both_failures_remain_available_for_a_skipped_plugin_notice() {
    let root = temp_root("missing");
    let missing = root.join("missing").to_string_lossy().into_owned();
    let configured_dirs = vec![missing.clone()];

    let resolution = resolve_patch_dirs(
        Some(&configured_dirs),
        Err(anyhow::anyhow!("factory missing")),
    );

    assert!(resolution.dirs.is_empty());
    assert_eq!(resolution.configured_missing, vec![missing]);
    assert_eq!(
        resolution.discovery_error.as_deref(),
        Some("factory missing")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "実 sforzando CLAP と SFZ directory が要る"]
fn installed_sforzando_keeps_config_locations_when_discovery_is_unusable() {
    let plugin = std::env::var("CMRT_TEST_SFORZANDO_CLAP")
        .expect("CMRT_TEST_SFORZANDO_CLAP に sforzando CLAP のパスを設定すること");
    let sfz_dir = std::env::var("CMRT_TEST_SFORZANDO_SFZ_DIR")
        .expect("CMRT_TEST_SFORZANDO_SFZ_DIR に SFZ directory を設定すること");

    let resolution = resolve_sforzando_patch_dirs(&plugin, Some(&[sfz_dir]));

    // sforzando 2.1.2.4 は実測では `*.nonya` だけを宣言し filesystem location を
    // 返さない。config fallback を失わないことを実機で固定する。
    assert!(resolution.discovery_error.is_some());
    assert_eq!(resolution.dirs.len(), 1);
    assert!(resolution.dirs.iter().all(|dir| Path::new(dir).is_dir()));
}
