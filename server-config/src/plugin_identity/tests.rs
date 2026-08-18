use super::*;

#[test]
fn file_stem_drops_directory_and_extension() {
    assert_eq!(
        plugin_file_stem(r"C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap"),
        "Surge XT"
    );
    assert_eq!(
        plugin_file_stem("/Library/Audio/Plug-Ins/CLAP/Dexed.clap"),
        "Dexed"
    );
    assert_eq!(plugin_file_stem("  "), "");
}
