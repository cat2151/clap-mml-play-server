//! 実物の音色置き場を読むテスト。合成 fixture では出ない揺れ（ヘッダの書式・
//! 実在するカテゴリコード・cartridge の仕様）を、インストール済みの実データで確かめる。
//!
//! Vaporizer2 の `.vvp` の置き場は、本番と同じ経路で registry から読む
//! （[`vaporizer2_presets_dir`]）。見つからなければ黙って通さず落とす。Dexed の cartridge 置き場は環境変数で渡す（`#[ignore]`）:
//!
//! ```text
//! CMRT_TEST_DEXED_CARTRIDGES=%APPDATA%\DigitalSuburban\Dexed\Cartridges
//! cargo test -p cmrt-core patch_list -- --include-ignored
//! ```

use super::*;
use crate::AudioPluginInfo;
use cmrt_server_config::VAPORIZER2_PLUGIN_ID;

/// 本番と同じ経路（組み込みプロファイル → registry）で置き場を引く。無ければ panic（skip にしない）。
fn vaporizer2_presets_dir() -> String {
    let cfg = cmrt_server_config::ServerConfig::from_toml_str(
        r#"
output_midi = "output.mid"
output_wav  = "output.wav"
sample_rate = 48000
buffer_size = 512
"#,
    )
    .unwrap();
    cfg.patch_dirs_of("Vaporizer2").into_iter().next().expect(
        "registry の Vaporizer2 InstallPath が読めない（Vaporizer2 をインストールすること）",
    )
}

fn installed_vaporizer2_presets() -> (String, Vec<std::path::PathBuf>) {
    let dir = vaporizer2_presets_dir();
    let patches = collect_patches(&dir).unwrap();
    assert!(
        !patches.is_empty(),
        "プリセットが 1 件も見つからない: {dir}"
    );
    (dir, patches)
}

/// 実物のプリセット置き場が丸ごと列挙できること。
/// ディスク上の `.vvp` の実ファイル数と突き合わせる。
#[test]
fn installed_vaporizer2_presets_are_all_listed() {
    let (dir, patches) = installed_vaporizer2_presets();

    // ディレクトリを再帰で走査して数えた「`.vvp` の実ファイル数」と一致すること。
    let on_disk = count_vvp_files(std::path::Path::new(&dir));
    assert_eq!(patches.len(), on_disk);
    assert!(
        patches
            .iter()
            .all(|path| crate::vvp::is_vvp_patch_path(&path.to_string_lossy())),
        "`.vvp` 以外が混ざっている"
    );
}

fn count_vvp_files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .map(|path| {
            if path.is_dir() {
                count_vvp_files(&path)
            } else {
                usize::from(
                    path.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("vvp")),
                )
            }
        })
        .sum()
}

/// 実物のプリセットが 1 件残らずヘッダを読めること。
/// 合成 fixture では実在するヘッダの揺れ（BOM・改行・属性順）を検出できない。
#[test]
fn every_installed_vaporizer2_preset_header_is_readable() {
    let (_, patches) = installed_vaporizer2_presets();

    let unreadable: Vec<String> = patches
        .iter()
        .filter_map(|path| crate::vvp::read_vvp_header(path).err())
        .map(|e| format!("{e:#}"))
        .collect();

    eprintln!(
        "{} 件中 {} 件のヘッダを読めた",
        patches.len(),
        patches.len() - unreadable.len()
    );
    assert!(
        unreadable.is_empty(),
        "ヘッダを読めないプリセット {} 件:\n{}",
        unreadable.len(),
        unreadable.join("\n")
    );
}

/// 実物のプリセットが 1 件残らず、カテゴリコード表にある selector カテゴリへ落ちること。
/// 表に無いコードは `selector_category` が `None` になり、`sort.category` に生の 2 文字が残る。
#[test]
fn every_installed_vaporizer2_preset_lands_in_a_known_category() {
    let (dir, patches) = installed_vaporizer2_presets();
    let vaporizer = AudioPluginInfo::new(
        "Vaporizer2",
        "Vaporizer2.clap",
        Some(VAPORIZER2_PLUGIN_ID.to_string()),
        None,
    );

    let uncategorized: Vec<String> = patches
        .iter()
        .filter_map(|path| {
            let display = to_relative(&dir, path);
            let patch = vaporizer.describe_patch(&display, Some(path));
            patch
                .selector_category
                .is_none()
                .then(|| format!("{display} (code: {})", patch.sort.category))
        })
        .collect();

    eprintln!(
        "{} 件中 {} 件が表にあるカテゴリへ落ちた",
        patches.len(),
        patches.len() - uncategorized.len()
    );
    assert!(
        uncategorized.is_empty(),
        "表に無いカテゴリコードのプリセット {} 件:\n{}",
        uncategorized.len(),
        uncategorized.join("\n")
    );
}

/// 実物の cartridge が 1 件残らず読めること。合成 fixture では仕様の読み違いを検出できない。
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
