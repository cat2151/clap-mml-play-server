use super::*;
use plugin_presets::surge_fx::SurgeFxParamLayout;

struct Offline;
impl FactoryFetcher for Offline {
    fn get_text(&mut self, _: &str) -> Result<String> {
        anyhow::bail!("offline")
    }
    fn get_bytes(&mut self, _: &str) -> Result<Vec<u8>> {
        anyhow::bail!("offline")
    }
}

#[test]
fn rejects_ambiguous_versions_and_reuses_only_matching_cache_offline() {
    for version in ["", "1.3", "1.3.4-dev", "../main", "1.3.4.abc"] {
        assert!(release_tag(version).is_err());
    }
    let root = std::env::temp_dir().join(format!("cmrt_surge_source_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let cache = root.join("layout.json");
    let file = SurgeFxLayoutFile {
        format_version: 2,
        generator_version: PARAM_LAYOUT_GENERATOR_VERSION,
        plugin_version: "8.7.6".into(),
        source_tag: "release_xt_8.7.6".into(),
        source_revision: "1".repeat(40),
        sst_effects_revision: "2".repeat(40),
        layouts: vec![SurgeFxParamLayout {
            fx_type: 71,
            display_name: "Invented".into(),
            posy_offset: [0; 12],
            active: [false; 12],
            names: std::array::from_fn(|_| String::new()),
        }],
    };
    let bytes = serde_json::to_vec(&file).unwrap();
    std::fs::write(&cache, &bytes).unwrap();
    assert_eq!(
        prepare_for_version("8.7.6", &cache, &root, &mut Offline).unwrap(),
        bytes
    );
    assert!(prepare_for_version("8.7.5", &cache, &root, &mut Offline).is_err());
    assert_eq!(std::fs::read(&cache).unwrap(), bytes);
    std::fs::write(&cache, b"broken").unwrap();
    assert!(prepare_for_version("8.7.6", &cache, &root, &mut Offline).is_err());
    assert_eq!(std::fs::read(&cache).unwrap(), b"broken");
    std::fs::remove_dir_all(root).unwrap();
}

/// Explicit integration test. Writes only to the requested external directory.
#[test]
#[ignore = "requires GitHub and CMRT_TEST_SURGE_SOURCE_DIR"]
fn generates_from_official_release_and_reuses_offline() {
    let root = std::path::PathBuf::from(std::env::var("CMRT_TEST_SURGE_SOURCE_DIR").unwrap());
    std::fs::create_dir_all(&root).unwrap();
    let cache = root.join("generated.json");
    let missing = root.join("not-published.json");
    assert!(!missing.exists());
    let bytes = prepare_for_version("1.3.4", &missing, &root, &mut HttpFetcher::default()).unwrap();
    let file = decode_layout_file(&bytes).unwrap();
    assert!(!file.layouts.is_empty());
    std::fs::write(&cache, &bytes).unwrap();
    assert_eq!(
        prepare_for_version("1.3.4", &cache, &root, &mut Offline).unwrap(),
        bytes
    );
    eprintln!(
        "generated {} effects at {}",
        file.layouts.len(),
        cache.display()
    );
}
