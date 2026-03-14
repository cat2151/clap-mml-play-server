use super::*;
use std::path::Path;

#[test]
fn to_relative_strips_base_prefix() {
    let patches_dir = "/patches";
    let abs_path = Path::new("/patches/Pads/Pad 1.fxp");
    assert_eq!(to_relative(patches_dir, abs_path), "Pads/Pad 1.fxp");
}

#[test]
fn to_relative_returns_abs_when_not_under_base() {
    let patches_dir = "/other_patches";
    let abs_path = Path::new("/patches/Pad 1.fxp");
    let result = to_relative(patches_dir, abs_path);
    // strip_prefix 失敗時は絶対パスをそのまま返す
    assert!(result.contains("Pad 1.fxp"));
}

#[test]
fn to_relative_single_level() {
    let patches_dir = "/patches";
    let abs_path = Path::new("/patches/Pad 1.fxp");
    assert_eq!(to_relative(patches_dir, abs_path), "Pad 1.fxp");
}

#[test]
fn collect_patches_finds_fxp_files() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_basic");
    let sub_dir = tmp_dir.join("Category");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(sub_dir.join("Patch1.fxp"), b"fake fxp").unwrap();
    std::fs::write(sub_dir.join("NotPatch.txt"), b"not fxp").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();
    std::fs::remove_dir_all(&tmp_dir).ok();

    assert_eq!(patches.len(), 1);
    assert!(patches[0].to_string_lossy().ends_with("Patch1.fxp"));
}

#[test]
fn collect_patches_recurses_into_subdirs() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_recurse");
    let sub1 = tmp_dir.join("Pads");
    let sub2 = tmp_dir.join("Leads");
    std::fs::create_dir_all(&sub1).unwrap();
    std::fs::create_dir_all(&sub2).unwrap();
    std::fs::write(sub1.join("Pad1.fxp"), b"fake").unwrap();
    std::fs::write(sub2.join("Lead1.fxp"), b"fake").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();
    std::fs::remove_dir_all(&tmp_dir).ok();

    assert_eq!(patches.len(), 2);
}

#[test]
fn collect_patches_ignores_non_fxp_files() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_ignore");
    std::fs::create_dir_all(&tmp_dir).unwrap();
    std::fs::write(tmp_dir.join("patch.mid"), b"midi").unwrap();
    std::fs::write(tmp_dir.join("patch.wav"), b"wav").unwrap();
    std::fs::write(tmp_dir.join("patch.fxp"), b"fxp").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();
    std::fs::remove_dir_all(&tmp_dir).ok();

    assert_eq!(patches.len(), 1);
    assert!(patches[0].to_string_lossy().ends_with("patch.fxp"));
}

#[test]
fn collect_patches_returns_sorted() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_sorted");
    std::fs::create_dir_all(&tmp_dir).unwrap();
    std::fs::write(tmp_dir.join("b.fxp"), b"b").unwrap();
    std::fs::write(tmp_dir.join("a.fxp"), b"a").unwrap();
    std::fs::write(tmp_dir.join("c.fxp"), b"c").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();
    std::fs::remove_dir_all(&tmp_dir).ok();

    let names: Vec<String> = patches
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["a.fxp", "b.fxp", "c.fxp"]);
}

#[test]
fn collect_patches_empty_dir_returns_empty() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_empty");
    std::fs::create_dir_all(&tmp_dir).unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();
    std::fs::remove_dir_all(&tmp_dir).ok();

    assert!(patches.is_empty());
}

#[test]
fn collect_patches_missing_dir_returns_error() {
    let result = collect_patches("/nonexistent/path/that/does/not/exist");
    assert!(result.is_err());
}
