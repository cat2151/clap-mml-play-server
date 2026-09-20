use super::*;

/// chain 付きの MML を受け付けない経路は catalog を持たない。
#[test]
fn none_has_no_catalog_and_is_not_available() {
    let effects = EffectPlugins::none();

    assert!(!effects.is_available());
    assert!(effects.catalog().is_none());
}

/// 手で並べた catalog はそのまま見え、走査は起きない。
#[test]
fn with_catalog_exposes_the_given_catalog_without_scanning() {
    let plugin = AudioEffectPluginInfo::new(
        "Test FX",
        "/clap/does-not-exist.clap",
        "org.example.fx",
        "/presets/does-not-exist",
    );
    let effects =
        EffectPlugins::with_catalog(AudioEffectCatalog::with_entries(vec![plugin], Vec::new()));

    assert!(effects.is_available());
    let catalog = effects.catalog().unwrap();
    assert_eq!(catalog.plugins().len(), 1);
    assert_eq!(catalog.plugins()[0].json_key, "Test FX preset");
}

/// entry のロードに失敗しても表は壊れず、同じ plugin を引き直せる。
#[test]
fn a_failed_entry_load_is_reported_and_not_cached() {
    let plugin = AudioEffectPluginInfo::new(
        "Test FX",
        "/clap/does-not-exist.clap",
        "org.example.fx",
        "/presets/does-not-exist",
    );
    let effects = EffectPlugins::with_catalog(AudioEffectCatalog::default());

    assert!(effects.entry(&plugin).is_err());
    assert!(effects.entry(&plugin).is_err());
}
