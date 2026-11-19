use super::*;

#[test]
fn presets_live_under_the_data_path_per_product() {
    let data = std::path::Path::new("D:")
        .join("u-he")
        .join("Zebralette3.data");
    assert_eq!(
        preset_dirs_of(Some(data.to_string_lossy().into_owned()), "Zebralette3"),
        vec![data
            .join("Presets")
            .join("Zebralette3")
            .to_string_lossy()
            .into_owned()]
    );
}

#[test]
fn an_empty_or_missing_data_path_gives_no_dirs() {
    assert!(preset_dirs_of(Some("  ".to_string()), "Zebralette3").is_empty());
    assert!(preset_dirs_of(None, "Zebralette3").is_empty());
}

/// このマシンの registry から、実在する音色置き場が 1 つだけ返ること。
#[test]
#[ignore = "TyrellN6 のインストールが要る"]
fn the_installed_tyrelln6_presets_dir_is_read_from_the_registry() {
    assert_installed_presets_dir("TyrellN6");
}

#[test]
#[ignore = "Zebralette3 のインストールが要る"]
fn the_installed_zebralette3_presets_dir_is_read_from_the_registry() {
    assert_installed_presets_dir("Zebralette3");
}

fn assert_installed_presets_dir(product: &str) {
    let dirs = u_he_preset_dirs(product);
    eprintln!("{product} preset dirs={dirs:?}");
    assert_eq!(dirs.len(), 1);
    assert!(std::path::Path::new(&dirs[0]).is_dir(), "{}", dirs[0]);
}
