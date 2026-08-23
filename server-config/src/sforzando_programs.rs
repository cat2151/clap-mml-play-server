//! Sforzando program-source adapter.
//!
//! ARIA bank IDs, manifests, and the Windows registry stay here. Generic catalog callers reach
//! this implementation only through [`crate::resolve_patch_catalog`].

mod manifest;
mod user_bank;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::patch_catalog::{resolve_plain_directories, PatchCatalogResolution};

/// A program that ARIA can resolve from a canonical SFZ path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SforzandoProgramRef {
    pub sfz_path: PathBuf,
    pub bank_id: String,
    pub bank_version: String,
    pub program_name: String,
    /// Human-readable provenance for diagnostics (registry user bank or bank manifest).
    pub source: String,
}

/// Resolve one requested SFZ without guessing a program name from its filename.
pub fn resolve_sforzando_program(path: &Path) -> anyhow::Result<SforzandoProgramRef> {
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        anyhow::anyhow!(
            "SFZ path を canonicalize できない '{}': {error}",
            path.display()
        )
    })?;
    if !is_sfz(&canonical) {
        anyhow::bail!(
            "ARIA program は .sfz file でなければならない: '{}'",
            canonical.display()
        );
    }

    let user_lookup = user_bank::read_user_bank_source();
    if let Some(source) = user_lookup.source.as_ref() {
        if let Some(program) = source.program_for(&canonical) {
            return Ok(program);
        }
    }

    let mut inspected = Vec::new();
    let mut manifest_errors = Vec::new();
    for manifest_path in manifest::manifests_near(&canonical) {
        inspected.push(manifest_path.display().to_string());
        match manifest::read_manifest(&manifest_path) {
            Ok(parsed) => {
                if let Some(program) = parsed
                    .programs
                    .into_iter()
                    .find(|program| canonical_key(&program.sfz_path) == canonical_key(&canonical))
                {
                    return Ok(program);
                }
            }
            Err(error) => manifest_errors.push(format!("{}: {error:#}", manifest_path.display())),
        }
    }

    let registry = user_lookup
        .error
        .unwrap_or_else(|| "registry user bank の範囲外".to_string());
    let mut manifests = if inspected.is_empty() {
        "近傍に *.bank.xml がない".to_string()
    } else {
        format!("登録がない: {}", inspected.join(" / "))
    };
    if !manifest_errors.is_empty() {
        manifests.push_str(&format!("; parse error: {}", manifest_errors.join(" / ")));
    }
    anyhow::bail!(
        "ARIA program source を解決できない '{}': user bank {registry}; installed bank {manifests}",
        canonical.display()
    )
}

pub(super) fn resolve_catalog(
    configured: Option<&[String]>,
    include_registry_user_bank: bool,
) -> PatchCatalogResolution {
    let plain = resolve_plain_directories(configured);
    let mut roots = plain.dirs.iter().map(PathBuf::from).collect::<Vec<_>>();
    let mut notices = Vec::new();
    let user_lookup = if include_registry_user_bank {
        user_bank::read_user_bank_source()
    } else {
        user_bank::UserBankLookup {
            source: None,
            error: None,
        }
    };
    if let Some(error) = user_lookup.error.as_ref() {
        notices.push(format!("ARIA user bank: {error}"));
    }
    if let Some(source) = user_lookup.source.as_ref() {
        if !roots
            .iter()
            .any(|root| canonical_key(root) == canonical_key(&source.root))
        {
            roots.push(source.root.clone());
        }
    }

    let mut programs = BTreeMap::<String, SforzandoProgramRef>::new();
    let mut conflicts = HashSet::new();
    if let Some(source) = user_lookup.source.as_ref() {
        for path in collect_sfz_files(&source.root, &mut notices) {
            if let Some(program) = source.program_for(&path) {
                insert_program(&mut programs, &mut conflicts, program, &mut notices);
            }
        }
    }

    let mut seen_manifests = HashSet::new();
    for root in &roots {
        for manifest_path in manifest::manifests_near(root) {
            if !seen_manifests.insert(canonical_key(&manifest_path)) {
                continue;
            }
            match manifest::read_manifest(&manifest_path) {
                Ok(parsed) => {
                    notices.extend(parsed.diagnostics);
                    for program in parsed.programs {
                        if roots
                            .iter()
                            .any(|root| path_is_within(&program.sfz_path, root))
                        {
                            insert_program(&mut programs, &mut conflicts, program, &mut notices);
                        }
                    }
                }
                Err(error) => notices.push(format!(
                    "ARIA bank manifest を読めない '{}': {error:#}",
                    manifest_path.display()
                )),
            }
        }
    }

    let mut all_files = BTreeMap::new();
    for root in &roots {
        for path in collect_sfz_files(root, &mut notices) {
            all_files.entry(canonical_key(&path)).or_insert(path);
        }
    }
    let excluded = all_files
        .keys()
        .filter(|key| !programs.contains_key(*key))
        .count();
    if excluded > 0 {
        notices.push(format!(
            "ARIA program source に登録されていない SFZ を {excluded} 件 catalog から除外"
        ));
    }

    let mut resolved_patches = programs
        .values()
        .map(|program| program.sfz_path.clone())
        .collect::<Vec<_>>();
    resolved_patches.sort_by_key(|path| canonical_key(path));
    let source_error = resolved_patches
        .is_empty()
        .then(|| "ARIA program source からロード可能な SFZ を 1 件も解決できない".to_string());
    let mut dirs = roots
        .into_iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.dedup_by(|left, right| canonical_key(Path::new(left)) == canonical_key(Path::new(right)));

    PatchCatalogResolution {
        dirs,
        resolved_patches: Some(resolved_patches),
        configured_missing: plain.configured_missing,
        source_error,
        notices,
    }
}

fn insert_program(
    programs: &mut BTreeMap<String, SforzandoProgramRef>,
    conflicts: &mut HashSet<String>,
    program: SforzandoProgramRef,
    notices: &mut Vec<String>,
) {
    let key = canonical_key(&program.sfz_path);
    if conflicts.contains(&key) {
        return;
    }
    if let Some(existing) = programs.get(&key) {
        if existing.bank_id == program.bank_id
            && existing.bank_version == program.bank_version
            && existing.program_name == program.program_name
        {
            return;
        }
        notices.push(format!(
            "同じ SFZ に異なる ARIA program が競合したため除外 '{}': {} / {}",
            program.sfz_path.display(),
            existing.source,
            program.source
        ));
        programs.remove(&key);
        conflicts.insert(key);
        return;
    }
    programs.insert(key, program);
}

fn collect_sfz_files(root: &Path, notices: &mut Vec<String>) -> Vec<PathBuf> {
    fn visit(dir: &Path, files: &mut Vec<PathBuf>, notices: &mut Vec<String>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                notices.push(format!(
                    "SFZ directory を読めない '{}': {error}",
                    dir.display()
                ));
                return;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, files, notices);
            } else if is_sfz(&path) {
                if let Ok(canonical) = std::fs::canonicalize(&path) {
                    files.push(canonical);
                }
            }
        }
    }
    let mut files = Vec::new();
    visit(root, &mut files, notices);
    files
}

fn is_sfz(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sfz"))
}

pub(super) fn canonical_key(path: &Path) -> String {
    let key = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}

pub(super) fn path_is_within(path: &Path, root: &Path) -> bool {
    strip_prefix_portable(path, root).is_some()
}

pub(super) fn strip_prefix_portable(path: &Path, root: &Path) -> Option<PathBuf> {
    let mut path_components = path.components();
    for root_component in root.components() {
        let path_component = path_components.next()?;
        let equal = if cfg!(windows) {
            path_component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&root_component.as_os_str().to_string_lossy())
        } else {
            path_component == root_component
        };
        if !equal {
            return None;
        }
    }
    Some(path_components.collect())
}

#[cfg(test)]
mod tests;
