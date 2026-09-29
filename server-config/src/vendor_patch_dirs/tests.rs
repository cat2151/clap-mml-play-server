use super::*;

#[test]
fn vaporizer2_presets_live_under_the_install_path() {
    let install = std::path::Path::new("D:").join("Vaporizer2");
    assert_eq!(
        vaporizer2_presets_of(&install.to_string_lossy()),
        install.join("Presets").to_string_lossy()
    );
}

#[test]
fn every_extra_presets_folder_line_is_read() {
    let ini = "window-width = 920\n\
               extra-presets-folder = E:\\Floe\\presets\n\
               extra-presets-folder=D:\\more presets\n\
               presets-install-location = E:\\Floe\\presets\n\
               extra-libraries-folder = E:\\Floe\\libs\n";
    assert_eq!(
        floe_extra_presets_folders(ini),
        vec![
            r"E:\Floe\presets".to_string(),
            r"D:\more presets".to_string()
        ]
    );
}

#[test]
fn an_ini_without_extra_presets_folder_yields_nothing() {
    assert!(floe_extra_presets_folders("window-width = 920\n").is_empty());
}
