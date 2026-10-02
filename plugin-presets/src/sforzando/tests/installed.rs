//! 実機の sforzando と ARIA の registry が要るテスト（ignored）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::*;

/// user bank は `.sfz` を置くだけで増えるので件数は固定せず、一覧に無い `.sfz` が
/// どれも「鳴らないので外した」ものであることを確かめる。
#[test]
#[ignore = "実機の sforzando と ARIA の registry が要る"]
fn installed_catalog_explains_every_unlisted_user_program() {
    let plugin = std::env::var("CMRT_TEST_SFORZANDO_CLAP").unwrap();
    let user_root = registered_user_bank_dir();

    let (resolution, unplayable) =
        resolve_catalog_and_unplayable(RegistrySources::read(Path::new(&plugin).exists()));

    assert!(resolution.notices.is_empty(), "{:?}", resolution.notices);
    let (user, installed): (Vec<_>, Vec<_>) = resolution
        .resolved_patches
        .unwrap()
        .into_iter()
        .partition(|path| path.starts_with(&user_root));
    assert!(!user.is_empty());
    for path in &user {
        assert!(path.is_file() && is_sfz(path), "{path:?}");
    }
    let listed = user
        .iter()
        .map(|path| canonical_key(path))
        .collect::<HashSet<_>>();
    let mut unexplained = Vec::new();
    collect_sfz_files(&user_root, &mut |path| {
        let key = canonical_key(&crate::lexical_absolute(path).unwrap());
        if !listed.contains(&key) && !unplayable.contains(&key) {
            unexplained.push(path.to_path_buf());
        }
    });
    assert!(unexplained.is_empty(), "{unexplained:#?}");
    assert_eq!(installed.len(), EXPECTED_INSTALLED_BANK_PROGRAMS);
}

/// This machine: Free Sounds 54 + TableWarp2 (1 `.sfz` + 36 `.ariax`).
const EXPECTED_INSTALLED_BANK_PROGRAMS: usize = 91;

#[test]
#[ignore = "実機の sforzando と ARIA の registry が要る"]
fn installed_tablewarp2_lists_its_program_and_every_factory_preset() {
    let plugin = std::env::var("CMRT_TEST_SFORZANDO_CLAP").unwrap();
    let bank_dirs = registered_bank_dirs("TableWarp2");

    let resolution = resolve_installed_catalog(&plugin);

    let listed = resolution
        .resolved_patches
        .unwrap()
        .into_iter()
        .filter(|path| bank_dirs.iter().any(|dir| path_is_within(path, dir)))
        .collect::<Vec<_>>();
    let ariax = listed
        .iter()
        .filter(|path| crate::sforzando::ariax::is_ariax(path))
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

fn collect_sfz_files(dir: &Path, found: &mut impl FnMut(&Path)) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_sfz_files(&path, found);
        } else if is_sfz(&path) {
            found(&path);
        }
    }
}

#[test]
#[ignore = "実機の sforzando と ARIA の registry が要る"]
fn installed_roots_come_from_the_registry() {
    let plugin = std::env::var("CMRT_TEST_SFORZANDO_CLAP").unwrap();
    let expected = ["Free Sounds", "TableWarp2"]
        .into_iter()
        .flat_map(registered_bank_dirs)
        .collect::<Vec<_>>();
    assert!(expected.len() >= 2, "{expected:?}");

    let resolution = resolve_installed_catalog(&plugin);
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

    let roots = resolve_roots(RegistrySources::read(Path::new(&plugin).exists()));
    assert_eq!(roots.dirs, resolution.dirs);
}

fn resolve_installed_catalog(plugin: &str) -> SforzandoCatalog {
    resolve_catalog(RegistrySources::read(Path::new(plugin).exists()))
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
