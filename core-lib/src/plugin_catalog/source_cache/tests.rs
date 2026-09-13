use super::*;

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cmrt_server_catalog_sources_{label}_{}.json",
        std::process::id()
    ))
}

fn write_cache(path: &Path, dirs: &[&str]) {
    let dirs = dirs
        .iter()
        .map(|dir| format!(r#""{dir}""#))
        .collect::<Vec<_>>()
        .join(",");
    std::fs::write(
        path,
        format!(
            r#"{{
  "format_version": 1,
  "plugins": [{{
    "name": "Sforzando",
    "plugin_path": "C:/CLAP/sforzando.clap",
    "plugin_id": "com.Plogue Art et Technologie, Inc.sforzando",
    "base": "C:/SFZ",
    "dirs": [{dirs}],
    "source_notices": ["12 files excluded"]
  }}]
}}"#
        ),
    )
    .unwrap();
}

#[test]
fn loads_the_matching_sforzando_roots_and_notices() {
    let path = test_path("ready");
    write_cache(&path, &["C:/SFZ/User", "C:/SFZ/Banks"]);

    let loaded = load_sforzando_from(
        &path,
        "C:/CLAP/sforzando.clap",
        &["C:/SFZ/Banks".to_string(), "C:/SFZ/User".to_string()],
    )
    .unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(loaded.dirs.len(), 2);
    assert_eq!(loaded.notices, ["12 files excluded"]);
    assert!(loaded.resolved_patches.is_none());
}

#[test]
fn rejects_stale_roots_without_scanning() {
    let path = test_path("stale");
    write_cache(&path, &["C:/SFZ/Old"]);

    let error = load_sforzando_from(&path, "C:/CLAP/sforzando.clap", &["C:/SFZ/New".to_string()])
        .unwrap_err();
    let _ = std::fs::remove_file(&path);

    assert!(error.to_string().contains("rootと一致しません"));
}
