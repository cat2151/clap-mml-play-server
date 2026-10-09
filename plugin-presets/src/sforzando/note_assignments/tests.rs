use super::*;

fn notes(path: &std::path::Path) -> Vec<u8> {
    sfz_note_assignments(path)
        .unwrap()
        .into_iter()
        .map(|assignment| assignment.note)
        .collect()
}

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
    assert_eq!(notes(&path), vec![36, 42, 60, 70, 71, 72, 73, 90, 91]);
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
    assert_eq!(notes(&path), Vec::<u8>::new());
    std::fs::write(&path, "<global> sample=sample.wav\n<region> key=-1\n<region> key=128\n<region> lokey=126 hikey=130").unwrap();
    assert_eq!(notes(&path), vec![126, 127]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn names_prefer_key_label_then_single_range_labels_and_sample_across_inheritance_and_includes() {
    let root = fixture("names");
    let path = root.join("kit.sfz");
    std::fs::write(
        &path,
        r#"
        <control> label_key38=Snare Center
        #define $hat 42
        <group> group_label=Tom Low
        <region> sample=tom_v1.wav key=41
        <region> sample=tom_v2.wav key=41
        <group> group_label=snare-hit-rr1
        <region> sample=Samples\snare_a.wav key=38
        <region> sample=Samples\snare_b.wav key=40
        <region> sample=Samples\snare_c.wav key=40
        <region> sample=hat.wav key=$hat region_label=Closed Hat
        <region> sample=hat_release.wav key=43 trigger=release
        <group>
        #include "part.sfz"
        <region> sample=*sine key=50
        "#,
    )
    .unwrap();
    std::fs::write(
        root.join("part.sfz"),
        "<region> sample=909 Rim Shot.wav key=37",
    )
    .unwrap();
    let named: Vec<_> = sfz_note_assignments(&path)
        .unwrap()
        .into_iter()
        .map(|assignment| (assignment.note, assignment.name.unwrap()))
        .collect();
    assert_eq!(
        named,
        vec![
            (37, "909 Rim Shot".to_string()),
            (38, "Snare Center".to_string()),
            // The group label also covers 38 and 42, so it does not name the sound on 40.
            (40, "snare_b +1".to_string()),
            (41, "Tom Low".to_string()),
            (42, "Closed Hat".to_string()),
            (50, "*sine".to_string()),
        ]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn one_shot_notes_require_every_sounding_region_on_the_key_to_be_one_shot() {
    let root = fixture("one_shot");
    let path = root.join("kit.sfz");
    std::fs::write(
        &path,
        r#"
        <group> loop_mode=one_shot
        <region> sample=kick.wav key=36
        <region> sample=snare.wav key=38
        <region> sample=snare_ring.wav key=38 loop_mode=no_loop
        <region> sample=hat.wav key=42 loopmode=ONE_SHOT
        <region> sample=hat_release.wav key=42 loop_mode=no_loop trigger=release
        <group>
        <region> sample=crash.wav key=49
        "#,
    )
    .unwrap();
    // 38 has a regular region beside the one-shot one; the release region on 42 does not count.
    assert_eq!(sfz_one_shot_notes(&path).unwrap(), vec![36, 42]);
    std::fs::remove_dir_all(root).unwrap();
}
