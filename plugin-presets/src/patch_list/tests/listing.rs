use super::*;
use crate::dexed::test_cartridge_bytes_with_params;

/// 診断は stderr へ出す。
fn collect_patch_listing(patches_dir: &str) -> Result<Vec<CollectedPatch>> {
    super::super::collect_patch_listing(patches_dir, |message| eprintln!("{message}"))
}

/// A と B の 2 cartridge。B の 00/01 は A の 00/01 と同じ音（00 は名前だけ違う）。
/// 残りの program は名前もパラメータも既定値のままなので、すべて同じ音になる。
fn write_two_cartridges(name: &str) -> PathBuf {
    let tmp_dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).unwrap();
    std::fs::write(
        tmp_dir.join("A.syx"),
        test_cartridge_bytes_with_params(&[(0, "BELL", 1), (1, "PAD", 2)]),
    )
    .unwrap();
    std::fs::write(
        tmp_dir.join("B.syx"),
        test_cartridge_bytes_with_params(&[(0, "BELL 2", 1), (1, "PAD", 2)]),
    )
    .unwrap();
    std::fs::write(tmp_dir.join("surge.fxp"), b"dummy").unwrap();
    tmp_dir
}

#[test]
fn listing_merges_same_voices_into_the_first_path() {
    let tmp_dir = write_two_cartridges("cmrt_test_patch_listing_merge");
    let base = tmp_dir.to_str().unwrap();

    let listing = collect_patch_listing(base).unwrap();

    let summary: Vec<(String, Option<MergedPatches>)> = listing
        .into_iter()
        .map(|patch| (to_relative(base, &patch.path), patch.merged))
        .collect();
    let merged = |count, names: &[&str]| {
        Some(MergedPatches {
            count,
            names: names.iter().map(|name| name.to_string()).collect(),
        })
    };
    assert_eq!(
        summary,
        vec![
            ("A.syx/00 BELL".to_string(), merged(2, &["BELL", "BELL 2"])),
            ("A.syx/01 PAD".to_string(), merged(2, &["PAD"])),
            ("A.syx/02 (no name)".to_string(), merged(60, &["(no name)"])),
            ("surge.fxp".to_string(), None),
        ]
    );
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn collect_patches_keeps_every_program_even_when_voices_are_the_same() {
    let tmp_dir = write_two_cartridges("cmrt_test_patch_listing_unmerged");

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();

    assert_eq!(patches.len(), 65);
    let _ = std::fs::remove_dir_all(&tmp_dir);
}
