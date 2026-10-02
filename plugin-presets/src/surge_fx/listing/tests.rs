use std::path::PathBuf;

use super::*;

#[test]
fn surge_classification_follows_the_folder_table() {
    assert_eq!(
        surge_classification("Reverb 2/Cathedral.srgfx", "Surge XT Effects"),
        ("Space / Imaging".to_string(), "Reverb".to_string())
    );
    assert_eq!(
        surge_classification("Airwindows/Filter/Air.srgfx", "Surge XT Effects"),
        ("Filter / EQ".to_string(), "Filter".to_string())
    );
    assert_eq!(
        surge_classification("Combulator/Sparkle.srgfx", "Surge XT Effects"),
        ("Filter / EQ".to_string(), "Filter".to_string())
    );
    assert_eq!(
        surge_classification("Conditioner/Limiter 1.srgfx", "Surge XT Effects"),
        ("Dynamics".to_string(), "Limiter / Clipper".to_string())
    );
    assert_eq!(
        surge_classification("Reverb 1/Hall.srgfx", "Surge XT Effects"),
        ("Space / Imaging".to_string(), "Reverb".to_string())
    );
}

#[test]
fn unknown_folder_becomes_its_own_category_and_kind() {
    assert_eq!(
        surge_classification("Foo/x.srgfx", "Surge XT Effects"),
        ("Foo".to_string(), "Foo".to_string())
    );
    assert_eq!(
        surge_classification("Airwindows/New/x.srgfx", "Surge XT Effects"),
        ("Airwindows/New".to_string(), "Airwindows/New".to_string())
    );
    assert_eq!(
        surge_classification("x.srgfx", "Surge XT Effects"),
        (
            "Surge XT Effects".to_string(),
            "Surge XT Effects".to_string()
        )
    );
}

#[test]
fn folder_table_has_no_duplicate_keys() {
    let mut folders: Vec<&str> = SURGE_FOLDER_CLASSIFICATION
        .iter()
        .map(|(folder, _, _)| *folder)
        .collect();
    let original_len = folders.len();
    folders.sort();
    folders.dedup();
    assert_eq!(folders.len(), original_len);
}

/// 実 install の `fx_presets` フォルダが無い環境（Linux CI など）では何もせず通す。
#[test]
fn installed_surge_presets_all_hit_the_table() {
    let root = PathBuf::from(r"C:\ProgramData\Surge XT\fx_presets");
    if !root.is_dir() {
        return;
    }
    let mut files = Vec::new();
    collect_srgfx_files(&root, &mut files);
    let mut unmatched = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let Some((folder, _)) = relative.rsplit_once('/') else {
            continue;
        };
        let hit = SURGE_FOLDER_CLASSIFICATION
            .iter()
            .any(|(entry_folder, _, _)| *entry_folder == folder);
        if !hit {
            unmatched.push(folder.to_string());
        }
    }
    assert!(unmatched.is_empty(), "表に無いフォルダ: {unmatched:?}");
}

#[cfg(test)]
fn collect_srgfx_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_srgfx_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("srgfx") {
            out.push(path);
        }
    }
}
