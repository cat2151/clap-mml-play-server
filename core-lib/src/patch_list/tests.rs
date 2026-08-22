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

#[test]
fn collect_patches_expands_a_cartridge_into_thirty_two_programs() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_cartridge");
    let sub_dir = tmp_dir.join("SynprezFM");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&sub_dir).unwrap();
    let cartridge = sub_dir.join("SynprezFM_01.syx");
    std::fs::write(
        &cartridge,
        crate::dx7::test_cartridge_bytes(&[(0, "Say Again."), (31, "LAST      ")]),
    )
    .unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();

    assert_eq!(patches.len(), 32);
    assert_eq!(
        to_relative(tmp_dir.to_str().unwrap(), &patches[0]),
        "SynprezFM/SynprezFM_01.syx/00 Say Again."
    );
    assert_eq!(
        to_relative(tmp_dir.to_str().unwrap(), &patches[31]),
        "SynprezFM/SynprezFM_01.syx/31 LAST"
    );
    // cartridge ファイル自身は音色ではないので一覧に出さない。
    assert!(!patches.iter().any(|path| path == &cartridge));
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn collect_patches_skips_a_broken_cartridge_but_keeps_the_rest() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_broken_cartridge");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).unwrap();
    std::fs::write(tmp_dir.join("broken.syx"), b"not a bulk dump").unwrap();
    std::fs::write(
        tmp_dir.join("good.syx"),
        crate::dx7::test_cartridge_bytes(&[(0, "Init")]),
    )
    .unwrap();
    std::fs::write(tmp_dir.join("surge.fxp"), b"dummy").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();

    assert_eq!(patches.len(), 33);
    assert!(patches.iter().any(|path| path.ends_with("surge.fxp")));
    assert!(patches
        .iter()
        .all(|path| !path.to_string_lossy().contains("broken.syx")));
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn collect_patches_matches_the_extension_case_insensitively() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_ext_case");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).unwrap();
    std::fs::write(
        tmp_dir.join("upper.SYX"),
        crate::dx7::test_cartridge_bytes(&[(0, "Init")]),
    )
    .unwrap();
    std::fs::write(tmp_dir.join("upper.FXP"), b"dummy").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();

    assert_eq!(patches.len(), 33);
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn collect_patches_lists_a_vvp_file_as_one_patch() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_vvp");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    let sub_dir = tmp_dir.join("Presets");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(sub_dir.join("AR Accent Arp.vvp"), b"<VASTvaporizer2/>").unwrap();
    std::fs::write(sub_dir.join("BA Deep.VVP"), b"<VASTvaporizer2/>").unwrap();
    std::fs::write(sub_dir.join("notes.txt"), b"not a patch").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();

    // `.fxp` と同じく 1 ファイル = 1 音色。`.syx` のような 32 program 展開はしない。
    assert_eq!(patches.len(), 2);
    assert_eq!(
        to_relative(tmp_dir.to_str().unwrap(), &patches[0]),
        "Presets/AR Accent Arp.vvp"
    );
    assert_eq!(
        to_relative(tmp_dir.to_str().unwrap(), &patches[1]),
        "Presets/BA Deep.VVP"
    );
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

/// 3 種別が同じディレクトリに混ざっていても、それぞれの数え方で 1 つの一覧になる。
#[test]
fn collect_patches_mixes_all_three_patch_forms() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_three_forms");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).unwrap();
    std::fs::write(tmp_dir.join("surge.fxp"), b"dummy").unwrap();
    std::fs::write(
        tmp_dir.join("dexed.syx"),
        crate::dx7::test_cartridge_bytes(&[(0, "Init")]),
    )
    .unwrap();
    std::fs::write(tmp_dir.join("vapor.vvp"), b"<VASTvaporizer2/>").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();

    // 1 + 32 + 1
    assert_eq!(patches.len(), 34);
    assert!(patches.iter().any(|path| path.ends_with("surge.fxp")));
    assert!(patches.iter().any(|path| path.ends_with("vapor.vvp")));
    // cartridge ファイル自身は音色ではない（`.vvp` はファイルそのものが音色）。
    assert!(!patches.iter().any(|path| path.ends_with("dexed.syx")));
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

/// 実物のプリセット置き場が丸ごと列挙できること。
/// 資料の実測（460 件・フラット・すべて `.vvp`）と突き合わせる。
///
/// ```text
/// CMRT_TEST_VAPORIZER2_PRESETS=N:\app4HDD\Vaporizer2\Presets
/// ```
#[test]
#[ignore = "実物の Vaporizer2 プリセット置き場が要る"]
fn installed_vaporizer2_presets_are_all_listed() {
    let Ok(dir) = std::env::var("CMRT_TEST_VAPORIZER2_PRESETS") else {
        panic!("CMRT_TEST_VAPORIZER2_PRESETS が未設定");
    };

    let patches = collect_patches(&dir).unwrap();

    assert!(
        !patches.is_empty(),
        "プリセットが 1 件も見つからない: {dir}"
    );
    // ディレクトリを走査して数えた「`.vvp` の実ファイル数」と一致すること。
    let on_disk = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("vvp"))
        })
        .count();
    assert_eq!(patches.len(), on_disk);
    assert!(
        patches
            .iter()
            .all(|path| crate::vvp::is_vvp_patch_path(&path.to_string_lossy())),
        "`.vvp` 以外が混ざっている"
    );
}

/// 実物の cartridge が 1 件残らず読めること。合成 fixture では仕様の読み違いを検出できない。
///
/// ```text
/// CMRT_TEST_DEXED_CARTRIDGES=C:\Users\...\DigitalSuburban\Dexed\Cartridges
/// ```
#[test]
#[ignore = "実物の cartridge ディレクトリが要る"]
fn installed_cartridges_all_parse() {
    let Ok(dir) = std::env::var("CMRT_TEST_DEXED_CARTRIDGES") else {
        panic!("CMRT_TEST_DEXED_CARTRIDGES が未設定");
    };

    let patches = collect_patches(&dir).unwrap();

    assert!(
        !patches.is_empty(),
        "cartridge が 1 件も見つからない: {dir}"
    );
    assert_eq!(
        patches.len() % 32,
        0,
        "読めなかった cartridge がある（stderr を見ること）: {} 件",
        patches.len()
    );
    for patch in &patches {
        let relative = to_relative(&dir, patch);
        crate::dx7::parse_cartridge_patch_path(&relative)
            .unwrap_or_else(|error| panic!("{relative}: {error:#}"));
    }
}
