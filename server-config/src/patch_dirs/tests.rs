use super::*;

fn dirs(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn shared_patch_root_dir_returns_single_dir_as_is() {
    let base = shared_patch_root_dir(&dirs(&["/tmp/surge-data/patches_factory"]));

    assert_eq!(base.as_deref(), Some("/tmp/surge-data/patches_factory"));
}

#[test]
fn shared_patch_root_dir_returns_common_parent() {
    let base = shared_patch_root_dir(&dirs(&[
        "/tmp/surge-data/patches_factory",
        "/tmp/surge-data/patches_3rdparty",
    ]));

    assert_eq!(base.as_deref(), Some("/tmp/surge-data"));
}

#[test]
fn shared_patch_root_dir_is_none_without_dirs() {
    assert_eq!(shared_patch_root_dir(&[]), None);
}

/// 空文字だけの要素は「書かれていない」と同じ扱いにする。落とさないと
/// 共通の親が根まで登ってしまい、patches_dir がとんでもない場所を指す。
#[test]
fn configured_patch_dirs_drops_blank_entries() {
    let configured =
        configured_patch_dirs(Some(&dirs(&["  ", "/tmp/surge-data/patches_factory", ""])));

    assert_eq!(configured, dirs(&["/tmp/surge-data/patches_factory"]));
}

#[test]
fn patch_root_dir_folds_configured_dirs_into_one() {
    let root = patch_root_dir(Some(&dirs(&[
        "/tmp/surge-data/patches_factory",
        "/tmp/surge-data/patches_3rdparty",
    ])));

    assert_eq!(root.as_deref(), Some("/tmp/surge-data"));
}

#[test]
fn patch_root_dir_is_none_when_patches_dirs_is_absent() {
    assert_eq!(patch_root_dir(None), None);
}
