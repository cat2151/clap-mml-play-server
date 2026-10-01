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

fn installed(product: &str, bank_paths: &[PathBuf]) -> installed_bank::AriaProduct {
    installed_bank::AriaProduct {
        product: product.to_string(),
        bank_paths: bank_paths.to_vec(),
    }
}

fn installed_only(products: Vec<installed_bank::AriaProduct>) -> PatchCatalogResolution {
    resolve_catalog(RegistrySources::fixture(None, products))
}

#[test]
fn user_bank_uses_canonical_containment_and_relative_program_name() {
    let root = TempRoot::new("user_bank");
    let sfz = root.0.join("Orchestra").join("Flute.SFZ");
    write(&sfz, b"<region>");
    let canonical_root = crate::lexical_absolute(&root.0).unwrap();
    let canonical_sfz = crate::lexical_absolute(&sfz).unwrap();
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
    let bank_path = root.0.join("Free Sounds.bank.xml");
    write(
        &bank_path,
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

    let resolution = installed_only(vec![installed("Free Sounds", &[bank_path])]);

    let paths = resolution.resolved_patches.as_ref().unwrap();
    assert_eq!(*paths, vec![crate::lexical_absolute(good).unwrap()]);
    assert_eq!(
        resolution.dirs,
        vec![crate::lexical_absolute(&root.0)
            .unwrap()
            .to_string_lossy()
            .into_owned()]
    );
    assert_no_excluded_notice(&resolution);
    assert!(resolution.notices.is_empty(), "{:?}", resolution.notices);
}

#[test]
fn product_without_banks_is_ignored_silently() {
    let resolution = installed_only(vec![installed("sforzando", &[])]);

    assert!(resolution.dirs.is_empty());
    assert!(resolution.notices.is_empty(), "{:?}", resolution.notices);
}

#[test]
fn registered_bank_path_that_is_gone_becomes_a_notice() {
    let root = TempRoot::new("missing_bank");
    let bank_path = root.0.join("Gone").join("Gone.bank.xml");

    let resolution = installed_only(vec![installed(
        "Gone Product",
        std::slice::from_ref(&bank_path),
    )]);

    assert!(resolution.dirs.is_empty());
    assert_eq!(resolution.notices.len(), 1, "{:?}", resolution.notices);
    let notice = &resolution.notices[0];
    assert!(notice.contains("Gone Product"), "{notice}");
    assert!(
        notice.contains(&bank_path.display().to_string()),
        "{notice}"
    );
}

#[test]
fn user_bank_and_installed_bank_roots_are_both_scanned() {
    let user_root = TempRoot::new("both_user");
    let user_sfz = user_root.0.join("Mine.sfz");
    write(&user_sfz, b"<region>");
    let bank_root = TempRoot::new("both_bank");
    let bank_sfz = bank_root.0.join("Programs").join("Synth.sfz");
    write(&bank_sfz, b"<region>");
    let bank_path = bank_root.0.join("Synth.bank.xml");
    write(
        &bank_path,
        manifest("3103", "1000", &[("Synth", "Programs/Synth.sfz")]).as_bytes(),
    );
    let user = user_bank::UserBankSource::fixture(
        crate::lexical_absolute(&user_root.0).unwrap(),
        "5000",
        "1000",
    );

    let resolution = resolve_catalog(RegistrySources::fixture(
        Some(user),
        vec![installed("Synth", &[bank_path])],
    ));

    assert_eq!(resolution.dirs.len(), 2, "{:?}", resolution.dirs);
    let mut expected = vec![
        crate::lexical_absolute(user_sfz).unwrap(),
        crate::lexical_absolute(bank_sfz).unwrap(),
    ];
    expected.sort_by_key(|path| canonical_key(path));
    assert_eq!(resolution.resolved_patches.unwrap(), expected);
}

fn assert_no_excluded_notice(resolution: &PatchCatalogResolution) {
    assert!(
        !resolution
            .notices
            .iter()
            .any(|notice| notice.contains("除外")),
        "{:?}",
        resolution.notices
    );
}

/// Files under `root`, which no `*.bank.xml` covers, as the catalog would pass them in.
fn loose_notice(root: &Path, names: &[&str]) -> String {
    let files = names.iter().map(|name| root.join(name)).collect::<Vec<_>>();
    let roots = [root.to_path_buf()];
    excluded::notice(
        &files,
        &excluded::ExcludedContext {
            roots: &roots,
            bank_roots: &HashSet::new(),
            user_root: None,
        },
    )
    .expect("one excluded notice")
}

#[test]
fn unregistered_files_inside_an_installed_bank_are_excluded_silently() {
    let root = TempRoot::new("include_parts");
    let programs = root.0.join("Programs");
    let kit = programs.join("CR-909");
    write(
        &kit.join("main.sfz"),
        b"<global>
#include \"BD.sfz\"
",
    );
    write(&kit.join("BD.sfz"), b"<region>");
    write(&programs.join("Xylophone.sfz"), b"<region>");
    let bank_path = root.0.join("Free Sounds.bank.xml");
    write(
        &bank_path,
        manifest("3102", "1001", &[("CR-909", "Programs/CR-909/main.sfz")]).as_bytes(),
    );

    let resolution = installed_only(vec![installed("Free Sounds", &[bank_path])]);

    assert_eq!(resolution.resolved_patches.as_ref().unwrap().len(), 1);
    assert_no_excluded_notice(&resolution);
}

#[test]
fn examples_are_capped_with_a_remaining_count() {
    let root = TempRoot::new("example_cap");

    let notice = loose_notice(&root.0, &["a.sfz", "b.sfz", "c.sfz"]);
    assert!(notice.contains("3 件"), "{notice}");
    assert!(notice.contains("a.sfz, b.sfz 他 1 件"), "{notice}");
}

#[test]
fn sfz_outside_any_program_source_points_to_the_user_files_directory() {
    let root = TempRoot::new("no_source");

    let notice = loose_notice(&root.0, &["Piano.sfz"]);
    assert!(notice.contains("user files directory"), "{notice}");
    assert!(notice.contains("Piano.sfz"), "{notice}");
    assert!(notice.contains("ロードできる"), "{notice}");
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
    let first = root.0.join("a.bank.xml");
    let second = root.0.join("b.bank.xml");
    write(
        &first,
        manifest("1", "1", &[("First", "Programs/shared.sfz")]).as_bytes(),
    );
    write(
        &second,
        manifest("2", "1", &[("Second", "Programs/shared.sfz")]).as_bytes(),
    );

    let resolution = installed_only(vec![
        installed("First", &[first]),
        installed("Second", &[second]),
    ]);

    assert!(resolution.resolved_patches.unwrap().is_empty());
    assert!(resolution
        .notices
        .iter()
        .any(|notice| notice.contains("競合")));
}

#[cfg(windows)]
mod installed;

mod ariax;

mod missing_samples;

mod included_parts;
