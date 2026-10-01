use std::path::{Path, PathBuf};

use super::*;

/// Windows では別ドライブ、それ以外では別ツリーの絶対パス。
fn abs(windows: &str, unix: &str) -> String {
    if cfg!(windows) {
        windows.to_string()
    } else {
        unix.to_string()
    }
}

fn sforzando_roots() -> Vec<String> {
    vec![
        abs(
            r"C:\Program Files\Plogue\TableWarp2",
            "/c/Plogue/TableWarp2",
        ),
        abs(r"D:\libs\Plogue\Free Sounds", "/d/libs/Plogue/Free Sounds"),
        abs(r"D:\libs\sfz", "/d/libs/sfz"),
    ]
}

fn joined(root: &str, parts: &[&str]) -> String {
    let mut path = PathBuf::from(root);
    path.extend(parts);
    path.to_string_lossy().into_owned()
}

#[test]
fn per_root_resolves_each_display_under_its_own_root() {
    let roots = sforzando_roots();
    let base = PatchBase::per_root(&roots);

    assert_eq!(
        base.resolve("TableWarp2/Programs/TableWarp2.sfz"),
        joined(&roots[0], &["Programs", "TableWarp2.sfz"])
    );
    assert_eq!(
        base.resolve("Free Sounds/Programs/Piano.sfz"),
        joined(&roots[1], &["Programs", "Piano.sfz"])
    );
    assert_eq!(
        base.resolve("sfz/VSCO-2-CE-1.1.0/Harp.sfz"),
        joined(&roots[2], &["VSCO-2-CE-1.1.0", "Harp.sfz"])
    );
}

#[test]
fn per_root_display_starts_with_the_root_folder_name_and_round_trips() {
    let roots = sforzando_roots();
    let base = PatchBase::per_root(&roots);

    for (root, expected) in [
        (&roots[0], "TableWarp2/Programs/TableWarp2.sfz"),
        (&roots[1], "Free Sounds/Programs/Piano.sfz"),
        (&roots[2], "sfz/VSCO-2-CE-1.1.0/Harp.sfz"),
    ] {
        let rest = expected.split('/').skip(1).collect::<Vec<_>>();
        let path = joined(root, &rest);

        let display = base.display(Path::new(&path));

        assert_eq!(display, expected);
        assert_eq!(base.resolve(&display), path);
    }
}

#[test]
fn per_root_does_not_fall_back_for_unknown_or_legacy_prefixes() {
    let base = PatchBase::per_root(&sforzando_roots());

    for patch in [
        "Plogue/Free Sounds/Programs/Piano.sfz",
        "Unknown/Piano.sfz",
        "Programs/Piano.sfz",
    ] {
        assert_eq!(base.resolve(patch), patch);
    }
}

#[test]
fn per_root_display_outside_every_root_is_the_absolute_path() {
    let base = PatchBase::per_root(&sforzando_roots());
    let outside = abs(r"D:\elsewhere\Piano.sfz", "/d/elsewhere/Piano.sfz");

    assert_eq!(base.display(Path::new(&outside)), outside);
}

#[test]
fn absolute_patch_is_returned_as_is() {
    let absolute = abs(r"D:\elsewhere\Piano.sfz", "/d/elsewhere/Piano.sfz");

    for base in [
        PatchBase::None,
        PatchBase::Shared(abs(r"C:\patches", "/patches")),
        PatchBase::per_root(&sforzando_roots()),
    ] {
        assert_eq!(base.resolve(&absolute), absolute);
    }
}

#[test]
fn shared_keeps_plain_join_and_strip_prefix() {
    let root = abs(r"C:\patches", "/patches");
    let base = PatchBase::Shared(root.clone());
    let path = joined(&root, &["Pads", "Pad 1.fxp"]);

    assert_eq!(
        base.resolve("Pads/Pad 1.fxp"),
        Path::new(&root)
            .join("Pads/Pad 1.fxp")
            .to_string_lossy()
            .into_owned()
    );
    assert_eq!(base.display(Path::new(&path)), "Pads/Pad 1.fxp");
    assert_eq!(base.scan_dir(), Some(root.as_str()));
}

#[test]
fn none_leaves_patches_untouched() {
    let base = PatchBase::None;

    assert_eq!(base.resolve("Pads/Pad 1.fxp"), "Pads/Pad 1.fxp");
    assert_eq!(base.scan_dir(), None);
}

#[test]
fn constructors_fall_back_to_none_without_dirs() {
    assert_eq!(PatchBase::per_root(&[]), PatchBase::None);
    assert_eq!(PatchBase::shared(&[]), PatchBase::None);
    assert_eq!(PatchBase::from(None), PatchBase::None);
    assert_eq!(
        PatchBase::from(Some("/patches".to_string())),
        PatchBase::Shared("/patches".to_string())
    );
    assert_eq!(PatchBase::per_root(&sforzando_roots()).scan_dir(), None);
}

#[cfg(windows)]
#[test]
fn display_matches_when_only_one_side_has_the_verbatim_prefix() {
    let path = Path::new(r"D:\libs\sfz\UI_METAL-GTX\Programs\Full.sfz");
    let verbatim_path = Path::new(r"\\?\D:\libs\sfz\UI_METAL-GTX\Programs\Full.sfz");
    let expected = "sfz/UI_METAL-GTX/Programs/Full.sfz";

    let verbatim_root = PatchBase::per_root(&[r"\\?\D:\libs\sfz".to_string()]);
    let plain_root = PatchBase::per_root(&[r"D:\libs\sfz".to_string()]);

    assert_eq!(verbatim_root.display(path), expected);
    assert_eq!(plain_root.display(verbatim_path), expected);
    assert_eq!(
        PatchBase::Shared(r"\\?\D:\libs".to_string()).display(path),
        expected
    );
}
