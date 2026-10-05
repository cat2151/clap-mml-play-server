use super::*;

fn fixture(name: &str) -> std::path::PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "cmrt_sfz_notes_{name}_{}_{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn all_note_on_ranges_keep_holes_inheritance_defines_includes_and_deduplicate() {
    let root = fixture("ranges");
    let path = root.join("kit.sfz");
    std::fs::write(
        &path,
        r#"
        #define $kick c2
        <global> sample=kick.wav key=$kick
        <region>
        <master> sample=snare.wav
        <group> key=42
        #include "part.sfz"
        <global> sample=wide.wav
        <region> lokey=70 hikey=73
        <region> key=71
        <region> key=100 trigger=release
        <region> key=101 trigger=release_key
        <region> key=90 trigger=first
        <region> key=91 trigger=legato
    "#,
    )
    .unwrap();
    std::fs::write(root.join("part.sfz"), "<region>\n<region> key=60").unwrap();
    assert_eq!(
        sfz_note_assignments(&path).unwrap(),
        vec![36, 42, 60, 70, 71, 72, 73, 90, 91]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn incomplete_includes_and_bad_keys_never_return_a_partial_set() {
    let root = fixture("failure");
    let path = root.join("kit.sfz");
    for tail in [
        "#include \"missing.sfz\"",
        "<region> sample=bad.wav key=$undefined",
        "#include \"kit.sfz\"",
    ] {
        std::fs::write(&path, format!("<region> sample=kick.wav key=36\n{tail}")).unwrap();
        assert!(sfz_note_assignments(&path).is_err(), "{tail}");
    }
    std::fs::write(&path, "<region> sample=off.wav key=36 trigger=release").unwrap();
    assert_eq!(sfz_note_assignments(&path).unwrap(), Vec::<u8>::new());
    std::fs::write(&path, "<global> sample=sample.wav\n<region> key=-1\n<region> key=128\n<region> lokey=126 hikey=130").unwrap();
    assert_eq!(sfz_note_assignments(&path).unwrap(), vec![126, 127]);
    std::fs::remove_dir_all(root).unwrap();
}
