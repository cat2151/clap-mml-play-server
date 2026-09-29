use super::TempRoot;
use super::*;

#[test]
fn sfz_whose_samples_are_all_missing_is_dropped_without_a_notice() {
    let root = TempRoot::new("missing_samples");
    write(&root.0.join("Samples").join("a.wav"), &[0; 4]);
    let top = root.0.join("Programs").join("Top.sfz");
    write(&top, br"<region> sample=..\Samples\a.wav");
    write(
        &root.0.join("Programs").join("Parts").join("Part.sfz"),
        br"<region> sample=..\Samples\a.wav",
    );
    let builtin = root.0.join("Programs").join("Sine.sfz");
    write(&builtin, b"<region> sample=*sine\n");
    let user =
        user_bank::UserBankSource::fixture(std::fs::canonicalize(&root.0).unwrap(), "5000", "1000");

    let resolution = resolve_catalog(RegistrySources::fixture(Some(user), Vec::new()));

    let mut expected = vec![
        std::fs::canonicalize(builtin).unwrap(),
        std::fs::canonicalize(top).unwrap(),
    ];
    expected.sort_by_key(|path| canonical_key(path));
    assert_eq!(resolution.resolved_patches.unwrap(), expected);
    assert!(resolution.notices.is_empty(), "{:?}", resolution.notices);
}
