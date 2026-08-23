use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

static TEMP_ID: AtomicUsize = AtomicUsize::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "cmrt_sforzando_programs_{name}_{}_{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

fn manifest(bank_id: &str, version: &str, programs: &[(&str, &str)]) -> String {
    let programs = programs
        .iter()
        .map(|(name, path)| {
            format!(
                r#"<AriaProgram name="{name}"><AriaElement id="0" path="{path}"/></AriaProgram>"#
            )
        })
        .collect::<String>();
    format!(
        r#"<?xml version="1.0"?><Key>ignored</Key><AriaBank id="{bank_id}" version="{version}">{programs}</AriaBank>"#
    )
}

#[test]
fn user_bank_uses_canonical_containment_and_relative_program_name() {
    let root = TempRoot::new("user_bank");
    let sfz = root.0.join("Orchestra").join("Flute.SFZ");
    write(&sfz, b"<region>");
    let canonical_root = std::fs::canonicalize(&root.0).unwrap();
    let canonical_sfz = std::fs::canonicalize(&sfz).unwrap();
    let source = user_bank::UserBankSource::fixture(canonical_root, "5000", "1000");

    let program = source.program_for(&canonical_sfz).unwrap();

    assert_eq!(program.bank_id, "5000");
    assert_eq!(program.bank_version, "1000");
    assert_eq!(program.program_name, "Orchestra/Flute");
    assert!(source.program_for(&root.0.join("notes.txt")).is_none());
}

#[test]
fn manifest_catalog_lists_only_declared_existing_sfz() {
    let root = TempRoot::new("manifest_catalog");
    let programs = root.0.join("Programs");
    let good = programs.join("Garritan").join("Glockenspiel.sfz");
    let helper = programs.join("Garritan").join("Xylophone.sfz");
    write(&good, b"<region>");
    write(&helper, b"<region>");
    write(
        &root.0.join("Free Sounds.bank.xml"),
        manifest(
            "3102",
            "1001",
            &[(
                "Garritan/Glockenspiel",
                "Programs/Garritan/Glockenspiel.sfz",
            )],
        )
        .as_bytes(),
    );
    let configured = vec![programs.to_string_lossy().into_owned()];

    let resolution = resolve_catalog(Some(&configured), false);

    let paths = resolution.resolved_patches.unwrap();
    assert_eq!(paths, vec![std::fs::canonicalize(good).unwrap()]);
    assert!(resolution
        .notices
        .iter()
        .any(|notice| notice.contains("1 件") && notice.contains("除外")));
}

#[test]
fn manifest_rejects_path_traversal_outside_the_bank_root() {
    let parent = TempRoot::new("traversal");
    let bank = parent.0.join("Bank");
    std::fs::create_dir_all(&bank).unwrap();
    let outside = parent.0.join("outside.sfz");
    write(&outside, b"<region>");
    let manifest_path = bank.join("bad.bank.xml");
    write(
        &manifest_path,
        manifest("12", "34", &[("Escape", "../outside.sfz")]).as_bytes(),
    );

    let parsed = manifest::read_manifest(&manifest_path).unwrap();

    assert!(parsed.programs.is_empty());
    assert!(parsed
        .diagnostics
        .iter()
        .any(|message| message.contains("root 外参照")));
}

#[test]
fn manifest_reports_missing_bank_and_program_attributes() {
    let root = TempRoot::new("missing_attributes");
    let missing_version = root.0.join("missing-version.bank.xml");
    write(
        &missing_version,
        br#"<AriaBank id="12"><AriaProgram name="Piano"/></AriaBank>"#,
    );

    let error = manifest::read_manifest(&missing_version)
        .err()
        .expect("missing version must fail");
    assert!(error.to_string().contains("AriaBank/@version"));

    let missing_program_fields = root.0.join("missing-program-fields.bank.xml");
    write(
        &missing_program_fields,
        br#"<AriaBank id="12" version="34"><AriaProgram><AriaElement path="Programs/a.sfz"/></AriaProgram><AriaProgram name="Piano"><AriaElement/></AriaProgram></AriaBank>"#,
    );

    let parsed = manifest::read_manifest(&missing_program_fields).unwrap();
    assert!(parsed.programs.is_empty());
    assert!(parsed
        .diagnostics
        .iter()
        .any(|message| message.contains("AriaProgram に name がない")));
    assert!(parsed
        .diagnostics
        .iter()
        .any(|message| message.contains("AriaElement に path がない")));
}

#[test]
fn conflicting_programs_for_one_canonical_path_are_excluded() {
    let root = TempRoot::new("conflict");
    let programs = root.0.join("Programs");
    write(&programs.join("shared.sfz"), b"<region>");
    write(
        &root.0.join("a.bank.xml"),
        manifest("1", "1", &[("First", "Programs/shared.sfz")]).as_bytes(),
    );
    write(
        &root.0.join("b.bank.xml"),
        manifest("2", "1", &[("Second", "Programs/shared.sfz")]).as_bytes(),
    );
    let configured = vec![programs.to_string_lossy().into_owned()];

    let resolution = resolve_catalog(Some(&configured), false);

    assert!(resolution.resolved_patches.unwrap().is_empty());
    assert!(resolution
        .notices
        .iter()
        .any(|notice| notice.contains("競合")));
}

#[test]
#[ignore = "実機の registry user bank と Free Sounds manifest が要る"]
fn installed_catalog_contains_583_loadable_programs() {
    let plugin = std::env::var("CMRT_TEST_SFORZANDO_CLAP").unwrap();
    let user = std::env::var("CMRT_TEST_SFORZANDO_PATCH_A")
        .unwrap()
        .parse::<PathBuf>()
        .unwrap();
    let free = std::env::var("CMRT_TEST_SFORZANDO_PATCH_B")
        .unwrap()
        .parse::<PathBuf>()
        .unwrap();
    assert!(user.is_file(), "{}", user.display());
    let programs_root = free
        .ancestors()
        .find(|ancestor| {
            ancestor
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("Programs"))
        })
        .expect("PATCH_B must be inside an installed bank Programs directory");
    let roots = vec![programs_root.to_string_lossy().into_owned()];
    let resolution =
        crate::resolve_patch_catalog(Some(crate::SFORZANDO_PLUGIN_ID), &plugin, Some(&roots));
    assert_eq!(resolution.resolved_patches.unwrap().len(), 583);
}
