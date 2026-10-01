//! 実機の sforzando と ARIA の registry が要るテスト（ignored）。

use std::path::{Path, PathBuf};

use super::TempRoot;
use super::*;

/// user bank は `.sfz` を置くだけで増えるので、その場で数えた件数と比べる。
#[test]
#[ignore = "実機の sforzando と ARIA の registry が要る"]
fn installed_catalog_lists_every_registered_program() {
    let plugin = std::env::var("CMRT_TEST_SFORZANDO_CLAP").unwrap();
    let user_root = registered_user_bank_dir();

    let resolution = crate::resolve_patch_catalog(Some(crate::SFORZANDO_PLUGIN_ID), &plugin, None);

    let (user, installed): (Vec<_>, Vec<_>) = resolution
        .resolved_patches
        .unwrap()
        .into_iter()
        .partition(|path| path.starts_with(&user_root));
    assert_eq!(
        user.len(),
        count_sfz_files(&user_root),
        "{:?}",
        resolution.notices
    );
    assert_eq!(
        installed.len(),
        EXPECTED_INSTALLED_BANK_PROGRAMS,
        "{:?}",
        resolution.notices
    );
}

/// This machine: Free Sounds 54 + TableWarp2 (1 `.sfz` + 36 `.ariax`).
const EXPECTED_INSTALLED_BANK_PROGRAMS: usize = 91;

#[test]
#[ignore = "実機の sforzando と ARIA の registry が要る"]
fn installed_tablewarp2_lists_its_program_and_every_factory_preset() {
    let plugin = std::env::var("CMRT_TEST_SFORZANDO_CLAP").unwrap();
    let bank_dirs = registered_bank_dirs("TableWarp2");

    let resolution = crate::resolve_patch_catalog(Some(crate::SFORZANDO_PLUGIN_ID), &plugin, None);

    let listed = resolution
        .resolved_patches
        .unwrap()
        .into_iter()
        .filter(|path| bank_dirs.iter().any(|dir| path_is_within(path, dir)))
        .collect::<Vec<_>>();
    let ariax = listed
        .iter()
        .filter(|path| crate::sforzando_programs::ariax::is_ariax(path))
        .count();
    assert_eq!(
        (listed.len() - ariax, ariax),
        (1, 36),
        "{listed:?} {:?}",
        resolution.notices
    );
    assert!(listed
        .iter()
        .any(|path| path.ends_with(Path::new("Keys").join("Airy Bells.ariax"))));
}

fn registered_user_bank_dir() -> PathBuf {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let path: String = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Plogue Art et Technologie, Inc\Aria")
        .unwrap()
        .get_value("user_files_dir")
        .unwrap();
    crate::lexical_absolute(path.trim()).unwrap()
}

fn count_sfz_files(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .map(|path| {
            if path.is_dir() {
                count_sfz_files(&path)
            } else {
                usize::from(
                    path.extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("sfz")),
                )
            }
        })
        .sum()
}

#[test]
#[ignore = "実機の sforzando と ARIA の registry が要る"]
fn installed_roots_come_from_the_registry_and_ignore_configured_dirs() {
    let plugin = std::env::var("CMRT_TEST_SFORZANDO_CLAP").unwrap();
    let expected = ["Free Sounds", "TableWarp2"]
        .into_iter()
        .flat_map(registered_bank_dirs)
        .collect::<Vec<_>>();
    assert!(expected.len() >= 2, "{expected:?}");

    let resolution = crate::resolve_patch_catalog(Some(crate::SFORZANDO_PLUGIN_ID), &plugin, None);
    let dirs = resolution
        .dirs
        .iter()
        .map(|dir| canonical_key(Path::new(dir)))
        .collect::<Vec<_>>();
    for dir in &expected {
        assert!(
            dirs.contains(&canonical_key(dir)),
            "{dir:?} not in {dirs:?}"
        );
    }
    assert!(
        !resolution
            .notices
            .iter()
            .any(|notice| notice.contains("installed bank")),
        "{:?}",
        resolution.notices
    );

    let missing = TempRoot::new("configured_missing").0.join("not-there");
    let configured = vec![missing.to_string_lossy().into_owned()];
    let plain = crate::resolve_patch_catalog_roots(Some(crate::SFORZANDO_PLUGIN_ID), &plugin, None);
    let with_toml = crate::resolve_patch_catalog_roots(
        Some(crate::SFORZANDO_PLUGIN_ID),
        &plugin,
        Some(&configured),
    );
    assert_eq!(plain, with_toml);
    assert_eq!(plain.dirs, resolution.dirs);
}

/// Bank directories read straight from `HKLM\SOFTWARE\<vendor>\<product>\Banks`.
fn registered_bank_dirs(product: &str) -> Vec<PathBuf> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let banks = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(format!(
            r"SOFTWARE\Plogue Art et Technologie, Inc\{product}\Banks"
        ))
        .unwrap_or_else(|error| panic!("{product} Banks: {error}"));
    banks
        .enum_keys()
        .map(|bank| {
            let path: String = banks
                .open_subkey(bank.unwrap())
                .unwrap()
                .get_value("bank_path")
                .unwrap();
            crate::lexical_absolute(Path::new(path.trim()).parent().unwrap()).unwrap()
        })
        .collect()
}
