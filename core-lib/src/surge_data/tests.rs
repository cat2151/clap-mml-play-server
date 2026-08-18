use std::{fs, path::Path, path::PathBuf};

use super::*;

/// テスト専用の作業ディレクトリ。前回の残骸があればリンクを辿らずに消してから作る。
fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("cmrt_surge_data_{name}"));
    if root.exists() || fs::symlink_metadata(&root).is_ok() {
        remove_entry(&root).unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_file(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Surge XT のデータディレクトリを模した最小構成を作る。
fn fake_source(root: &Path) {
    write_file(
        &root
            .join(PATCHES_FACTORY)
            .join(TEMPLATES)
            .join("Init Saw.fxp"),
        "init",
    );
    write_file(
        &root.join(PATCHES_FACTORY).join("Leads").join("Lead.fxp"),
        "lead",
    );
    write_file(
        &root.join(PATCHES_3RDPARTY).join("Vendor").join("Pad.fxp"),
        "pad",
    );
    write_file(&root.join(WAVETABLES).join("saw.wt"), "wt");
    write_file(&root.join("skins").join("skin.xml"), "skin");
    write_file(&root.join("readme.txt"), "readme");
}

#[test]
fn shared_entry_names_excludes_patch_directories() {
    let root = temp_root("shared_entry_names");
    let source = root.join("source");
    fake_source(&source);

    let names = shared_entry_names(&source).unwrap();

    assert_eq!(
        names,
        vec![
            std::ffi::OsString::from("readme.txt"),
            std::ffi::OsString::from("skins"),
            std::ffi::OsString::from(WAVETABLES),
        ],
        "patches 系だけを除いた最上位エントリが並ぶこと"
    );
    remove_entry(&root).unwrap();
}

#[test]
fn sync_minimal_tree_shares_data_and_drops_patches() {
    let root = temp_root("build_tree");
    let source = root.join("source");
    let minimal = root.join("minimal");
    fake_source(&source);

    assert!(
        sync_minimal_tree(&source, &minimal).unwrap(),
        "初回は作り直す"
    );

    assert_eq!(
        fs::read_to_string(minimal.join(WAVETABLES).join("saw.wt")).unwrap(),
        "wt",
        "wavetable は元データの実体が見えること"
    );
    assert_eq!(
        fs::read_to_string(minimal.join("readme.txt")).unwrap(),
        "readme",
        "ディレクトリ以外はコピーされること"
    );
    assert_eq!(
        fs::read_to_string(
            minimal
                .join(PATCHES_FACTORY)
                .join(TEMPLATES)
                .join("Init Saw.fxp")
        )
        .unwrap(),
        "init",
        "初期パッチは残すこと（空にすると出音が変わる）"
    );
    assert!(
        !minimal.join(PATCHES_FACTORY).join("Leads").exists(),
        "Templates 以外の factory patch は見えないこと"
    );
    assert!(
        entry_names(&minimal.join(PATCHES_3RDPARTY))
            .unwrap()
            .is_empty(),
        "patches_3rdparty は空ディレクトリであること"
    );
    assert_eq!(
        fs::read_to_string(minimal.join(SOURCE_MARKER_NAME))
            .unwrap()
            .trim(),
        source.display().to_string(),
        "元データの位置が記録されること"
    );
    remove_entry(&root).unwrap();
}

#[test]
fn sync_minimal_tree_skips_rebuild_when_current() {
    let root = temp_root("skip_rebuild");
    let source = root.join("source");
    let minimal = root.join("minimal");
    fake_source(&source);

    assert!(sync_minimal_tree(&source, &minimal).unwrap());
    assert!(
        !sync_minimal_tree(&source, &minimal).unwrap(),
        "2 回目は作り直さないこと"
    );
    remove_entry(&root).unwrap();
}

#[test]
fn sync_minimal_tree_rebuilds_when_source_directory_is_added() {
    let root = temp_root("rebuild_on_new_dir");
    let source = root.join("source");
    let minimal = root.join("minimal");
    fake_source(&source);
    sync_minimal_tree(&source, &minimal).unwrap();

    write_file(&source.join("tuning_library").join("scale.scl"), "scl");

    assert!(
        sync_minimal_tree(&source, &minimal).unwrap(),
        "Surge の更新で増えたディレクトリに追従すること"
    );
    assert!(minimal.join("tuning_library").join("scale.scl").exists());
    remove_entry(&root).unwrap();
}

#[test]
fn sync_minimal_tree_rebuilds_when_source_moves() {
    let root = temp_root("rebuild_on_source_move");
    let first = root.join("first");
    let second = root.join("second");
    let minimal = root.join("minimal");
    fake_source(&first);
    fake_source(&second);
    sync_minimal_tree(&first, &minimal).unwrap();

    assert!(
        sync_minimal_tree(&second, &minimal).unwrap(),
        "元データの位置が変わったら作り直すこと"
    );
    assert_eq!(
        fs::read_to_string(minimal.join(SOURCE_MARKER_NAME))
            .unwrap()
            .trim(),
        second.display().to_string()
    );
    remove_entry(&root).unwrap();
}

#[test]
fn sync_minimal_tree_fails_without_templates() {
    let root = temp_root("missing_templates");
    let source = root.join("source");
    let minimal = root.join("minimal");
    fake_source(&source);
    remove_entry(&source.join(PATCHES_FACTORY).join(TEMPLATES)).unwrap();

    let error = sync_minimal_tree(&source, &minimal).unwrap_err();

    assert!(
        error.to_string().contains("初期パッチ"),
        "Templates が無いことを理由に諦めること: {error}"
    );
    remove_entry(&root).unwrap();
}

/// 元データを巻き込んで削除しないことの確認。ここが壊れると実データが消えるので最重要。
#[test]
fn clear_dir_removes_links_without_touching_targets() {
    let root = temp_root("clear_dir_safety");
    let target = root.join("target");
    let holder = root.join("holder");
    write_file(&target.join("keep.txt"), "keep");
    fs::create_dir_all(&holder).unwrap();
    link_dir(&holder.join("linked"), &target).unwrap();
    write_file(&holder.join("real").join("scratch.txt"), "scratch");

    clear_dir(&holder).unwrap();

    assert!(
        entry_names(&holder).unwrap().is_empty(),
        "リンクも実ディレクトリも消えること"
    );
    assert_eq!(
        fs::read_to_string(target.join("keep.txt")).unwrap(),
        "keep",
        "リンク先の実体は残ること"
    );
    remove_entry(&root).unwrap();
}

#[test]
fn ensure_templates_not_empty_rejects_directory_without_patches() {
    let root = temp_root("empty_templates");
    fs::create_dir_all(root.join(PATCHES_FACTORY).join(TEMPLATES)).unwrap();

    let error = ensure_templates_not_empty(&root).unwrap_err();

    assert!(
        error.to_string().contains("出音が変わる"),
        "空の Templates を拒否すること: {error}"
    );
    remove_entry(&root).unwrap();
}

#[test]
fn looks_like_surge_data_home_requires_templates_and_wavetables() {
    let root = temp_root("looks_like");
    let source = root.join("source");
    fake_source(&source);

    assert!(looks_like_surge_data_home(&source));
    assert!(!looks_like_surge_data_home(&root.join("missing")));

    remove_entry(&source.join(WAVETABLES)).unwrap();
    assert!(
        !looks_like_surge_data_home(&source),
        "wavetables が無い構成は元データとして扱わないこと"
    );
    remove_entry(&root).unwrap();
}

#[test]
fn already_applied_reports_existing_minimal_data_home() {
    let root = temp_root("already_applied");
    let minimal = root.join("minimal");
    let source = root.join("source");
    fake_source(&source);
    sync_minimal_tree(&source, &minimal).unwrap();

    let _guard = crate::pipeline::EnvVarGuard::set(SURGE_DATA_HOME_ENV, &minimal);
    let applied = already_applied()
        .unwrap()
        .expect("最小ディレクトリを検出すること");

    assert_eq!(applied.path, minimal);
    assert_eq!(applied.source, source);
    assert!(!applied.rebuilt);
}

#[test]
fn already_applied_ignores_plain_data_home() {
    let root = temp_root("already_applied_plain");
    let source = root.join("source");
    fake_source(&source);

    let _guard = crate::pipeline::EnvVarGuard::set(SURGE_DATA_HOME_ENV, &source);

    assert!(
        already_applied().unwrap().is_none(),
        "目印のないディレクトリは元データとして扱うこと"
    );
}

/// config が plugin_id を持つなら、パスが何であれ ID だけで決まる。
#[test]
fn a_configured_plugin_id_decides_without_looking_at_the_path() {
    assert!(plugin_is_surge(
        Some(SURGE_XT_PLUGIN_ID),
        r"D:\my\clap\whatever.clap"
    ));
    assert!(!plugin_is_surge(
        Some("com.digital-suburban.dexed"),
        r"C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap"
    ));
}

/// plugin_id を持たない config（active_plugin を書いていない既定の config）では、
/// ファイル名からの推測へ落ちる。
#[test]
fn surge_plugin_paths_are_recognized_regardless_of_case() {
    assert!(plugin_is_surge(
        None,
        r"C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap"
    ));
    assert!(plugin_is_surge(None, "/usr/lib/clap/surge-xt.clap"));
    assert!(plugin_is_surge(None, "SURGE XT.CLAP"));
}

#[test]
fn other_plugin_paths_are_not_treated_as_surge() {
    assert!(!plugin_is_surge(
        None,
        r"C:\Program Files\Common Files\CLAP\Dexed.clap"
    ));
    assert!(!plugin_is_surge(None, ""));
    // ディレクトリ名だけが一致しても、プラグイン本体は Surge ではない。
    assert!(!plugin_is_surge(None, r"C:\Surge Synth Team\Dexed.clap"));
}
