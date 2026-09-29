use super::TempRoot;
use super::*;

#[test]
fn part_sfz_included_by_another_program_is_dropped_without_a_notice() {
    let root = TempRoot::new("included_parts");
    write(&root.0.join("Samples").join("a.wav"), &[0; 4]);
    let top = root.0.join("Kit").join("#Kit.sfz");
    write(
        &top,
        b"<control> set_cc24=13\n#include \"Part.sfz\"\n#include \"Group.sfz\"\n",
    );
    let part_region = br"<region> sample=..\Samples\a.wav locc24=2 hicc24=15";
    write(&root.0.join("Kit").join("Part.sfz"), part_region);
    write(
        &root.0.join("Kit").join("Group.sfz"),
        b"#include \"Nested.sfz\"\n",
    );
    write(&root.0.join("Kit").join("Nested.sfz"), part_region);
    let standalone = root.0.join("Kit").join("Standalone.sfz");
    write(&standalone, br"<region> sample=..\Samples\a.wav");
    let user =
        user_bank::UserBankSource::fixture(std::fs::canonicalize(&root.0).unwrap(), "5000", "1000");

    let resolution = resolve_catalog(RegistrySources::fixture(Some(user), Vec::new()));

    let mut expected = vec![
        std::fs::canonicalize(top).unwrap(),
        std::fs::canonicalize(standalone).unwrap(),
    ];
    expected.sort_by_key(|path| canonical_key(path));
    assert_eq!(resolution.resolved_patches.unwrap(), expected);
    assert!(resolution.notices.is_empty(), "{:?}", resolution.notices);
}

#[test]
fn sfz_that_includes_itself_stays_listed() {
    let root = TempRoot::new("self_include");
    write(&root.0.join("Samples").join("a.wav"), &[0; 4]);
    let program = root.0.join("Kit").join("Loop.sfz");
    write(
        &program,
        b"<region> sample=../Samples/a.wav\n#include \"Loop.sfz\"\n",
    );
    let user =
        user_bank::UserBankSource::fixture(std::fs::canonicalize(&root.0).unwrap(), "5000", "1000");

    let resolution = resolve_catalog(RegistrySources::fixture(Some(user), Vec::new()));

    assert_eq!(
        resolution.resolved_patches.unwrap(),
        vec![std::fs::canonicalize(program).unwrap()]
    );
}
