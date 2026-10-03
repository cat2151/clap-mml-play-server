use std::path::{Path, PathBuf};

use super::*;

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cmrt_test_sfz_regions_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, relative: &str, content: &str) -> PathBuf {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, content).unwrap();
    path
}

fn opcodes(region: &SfzRegion) -> Vec<(&str, &str)> {
    region
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

#[test]
fn regions_inherit_headers_and_a_new_group_drops_the_previous_group() {
    let dir = fixture_dir("inherit");
    let path = write(
        &dir,
        "kit.sfz",
        "<global> ampeg_release=1\n\
         <group> pitch_keytrack=0 lokey=36\n\
         <region> sample=kick a.wav hikey=37 // sample=commented.wav\n\
         <group>\n\
         <region>sample=snare.wav key=38 ampeg_release=2\n\
         <curve> v000=0\n",
    );

    let regions = sfz_regions(&path).unwrap();

    assert_eq!(
        regions.iter().map(opcodes).collect::<Vec<_>>(),
        vec![
            vec![
                ("ampeg_release", "1"),
                ("hikey", "37"),
                ("lokey", "36"),
                ("pitch_keytrack", "0"),
                ("sample", "kick a.wav"),
            ],
            vec![
                ("ampeg_release", "2"),
                ("key", "38"),
                ("sample", "snare.wav"),
            ],
        ]
    );
}

#[test]
fn defines_expand_after_definition_and_includes_resolve_from_the_root_dir() {
    let dir = fixture_dir("include");
    write(&dir, "parts/hat.sfz", "<region> sample=hat.wav key=$HAT\n");
    let path = write(
        &dir,
        "Programs/kit.sfz",
        "#define $HAT 42\n\
         <region> sample=kick.wav key=36\n\
         #include \"../parts/hat.sfz\"\n",
    );

    let regions = sfz_regions(&path).unwrap();

    assert_eq!(
        regions
            .iter()
            .map(|region| region["key"].as_str())
            .collect::<Vec<_>>(),
        vec!["36", "42"]
    );
}

#[test]
fn unreadable_root_is_an_error() {
    let dir = fixture_dir("missing");

    assert!(sfz_regions(&dir.join("missing.sfz")).is_err());
}
