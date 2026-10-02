use super::*;

#[test]
fn vaporizer2_presets_live_under_the_install_path() {
    let install = std::path::Path::new("D:").join("Vaporizer2");
    assert_eq!(
        vaporizer2_presets_of(&install.to_string_lossy()),
        install.join("Presets").to_string_lossy()
    );
}
