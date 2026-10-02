use super::*;

#[test]
fn tyrelln6_presets_live_under_the_data_path() {
    let data = std::path::Path::new("D:")
        .join("u-he")
        .join("TyrellN6.data");
    assert_eq!(
        tyrelln6_presets_of(&data.to_string_lossy()),
        data.join("Presets").join("TyrellN6").to_string_lossy()
    );
}

/// このマシンの registry から、実在する音色置き場が 1 つだけ返ること。
#[test]
#[ignore = "TyrellN6 のインストールが要る"]
fn the_installed_tyrelln6_presets_dir_is_read_from_the_registry() {
    let dirs = tyrelln6_preset_dirs();
    eprintln!("tyrelln6 preset dirs={dirs:?}");
    assert_eq!(dirs.len(), 1);
    assert!(std::path::Path::new(&dirs[0]).is_dir(), "{}", dirs[0]);
}
