//! Sforzando program-source adapter.
//!
//! ARIA bank IDs, manifests, and the Windows registry stay here. Generic catalog callers reach
//! this implementation only through [`crate::resolve_patch_catalog`].
//!
//! Scan roots come only from ARIA's registry (user bank + installed banks); a configured
//! `patches_dirs` is ignored. Sforzando's state names a program by bank coordinates, so a
//! directory ARIA has not registered could be listed but not played.

mod ariax;
mod excluded;
mod installed_bank;
mod manifest;
mod sample_weight;
mod user_bank;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::patch_catalog::PatchCatalogResolution;

pub use ariax::{resolve_sforzando_preset, SforzandoPresetRef};
pub use sample_weight::{sfz_sample_weight, SfzSampleWeight};

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
    let canonical = crate::lexical_absolute(path).map_err(|error| {
        anyhow::anyhow!(
            "SFZ path を絶対パスにできない '{}': {error}",
            path.display()
        )
    })?;
    if !canonical.is_file() {
        anyhow::bail!("SFZ file が無い: '{}'", canonical.display());
    }
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

/// What ARIA has registered. Empty when the plugin itself is absent.
pub(super) struct RegistrySources {
    user: user_bank::UserBankLookup,
    installed: installed_bank::InstalledBankLookup,
}

impl RegistrySources {
    pub(super) fn read(plugin_installed: bool) -> Self {
        if !plugin_installed {
            return Self {
                user: user_bank::UserBankLookup {
                    source: None,
                    error: None,
                },
                installed: installed_bank::InstalledBankLookup {
                    products: Vec::new(),
                    error: None,
                },
            };
        }
        Self {
            user: user_bank::read_user_bank_source(),
            installed: installed_bank::read_installed_banks(),
        }
    }

    #[cfg(test)]
    fn fixture(
        user: Option<user_bank::UserBankSource>,
        products: Vec<installed_bank::AriaProduct>,
    ) -> Self {
        Self {
            user: user_bank::UserBankLookup {
                source: user,
                error: None,
            },
            installed: installed_bank::InstalledBankLookup {
                products,
                error: None,
            },
        }
    }
}

pub(super) fn resolve_catalog(sources: RegistrySources) -> PatchCatalogResolution {
    let CatalogRoots {
        roots,
        mut notices,
        user_lookup,
    } = catalog_roots(sources);
    let mut programs = BTreeMap::<String, SforzandoProgramRef>::new();
    let mut conflicts = HashSet::new();
    if let Some(source) = user_lookup.source.as_ref() {
        for path in collect_patch_files(&source.root, &mut notices).sfz {
            if let Some(program) = source.program_for(&path) {
                insert_program(&mut programs, &mut conflicts, program, &mut notices);
            }
        }
    }

    let mut seen_manifests = HashSet::new();
    let mut bank_roots = HashSet::new();
    for root in &roots {
        for manifest_path in manifest::manifests_near(root) {
            bank_roots.insert(canonical_key(root));
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
    let mut ariax_files = BTreeMap::new();
    for root in &roots {
        let files = collect_patch_files(root, &mut notices);
        for path in files.sfz {
            all_files.entry(canonical_key(&path)).or_insert(path);
        }
        for path in files.ariax {
            ariax_files.entry(canonical_key(&path)).or_insert(path);
        }
    }
    let excluded = all_files
        .into_iter()
        .filter(|(key, _)| !programs.contains_key(key) && !conflicts.contains(key))
        .map(|(_, path)| path)
        .collect::<Vec<_>>();
    notices.extend(excluded::notice(
        &excluded,
        &excluded::ExcludedContext {
            roots: &roots,
            bank_roots: &bank_roots,
            user_root: user_lookup
                .source
                .as_ref()
                .map(|source| source.root.as_path()),
        },
    ));

    drop_unplayable_parts(&mut programs);

    let mut resolved_patches = programs
        .values()
        .map(|program| program.sfz_path.clone())
        .chain(ariax::listable_presets(ariax_files.into_values().collect()))
        .collect::<Vec<_>>();
    resolved_patches.sort_by_key(|path| canonical_key(path));
    let source_error = resolved_patches.is_empty().then(|| {
        "ARIA program source からロード可能な SFZ / .ariax を 1 件も解決できない".to_string()
    });

    PatchCatalogResolution {
        dirs: root_strings(roots),
        resolved_patches: Some(resolved_patches),
        source_error,
        notices,
        ..PatchCatalogResolution::default()
    }
}

/// Realtime startup fallback: resolve roots and diagnostics without walking SFZ files.
pub(super) fn resolve_roots(sources: RegistrySources) -> PatchCatalogResolution {
    let CatalogRoots { roots, notices, .. } = catalog_roots(sources);
    let dirs = root_strings(roots);
    let source_error = dirs
        .is_empty()
        .then(|| "ARIA program sourceのrootを1件も解決できない".to_string());
    PatchCatalogResolution {
        dirs,
        source_error,
        notices,
        ..PatchCatalogResolution::default()
    }
}

struct CatalogRoots {
    roots: Vec<PathBuf>,
    notices: Vec<String>,
    user_lookup: user_bank::UserBankLookup,
}

fn catalog_roots(sources: RegistrySources) -> CatalogRoots {
    let RegistrySources {
        user: user_lookup,
        installed,
    } = sources;
    let (mut roots, mut notices) = installed_bank::installed_bank_roots(&installed.products);
    if let Some(error) = installed.error.as_ref() {
        notices.push(format!("ARIA installed bank: {error}"));
    }
    if let Some(error) = user_lookup.error.as_ref() {
        notices.push(format!("ARIA user bank: {error}"));
    }
    if let Some(source) = user_lookup.source.as_ref() {
        roots.push(source.root.clone());
    }
    roots.sort_by_key(|root| canonical_key(root));
    roots.dedup_by(|left, right| canonical_key(left) == canonical_key(right));
    CatalogRoots {
        roots,
        notices,
        user_lookup,
    }
}

/// 単体で開いても鳴らない program を一覧から外す。user 側に対処がないので通知しない。
///
/// - 他の program に `#include` される部品。親の `<control>` の `set_ccN` や `sw_default` が
///   前提で、単体では CC が 0 のまま（`locc` 条件）や key switch 待ちになり、
///   どの note を送っても鳴らないことがある。音はどれも親から出せる
/// - sample がどれも見つからない sfz
fn drop_unplayable_parts(programs: &mut BTreeMap<String, SforzandoProgramRef>) {
    let mut included = HashSet::new();
    let mut all_samples_missing = HashSet::new();
    for (key, program) in programs.iter() {
        let Ok(scan) = sample_weight::scan_sfz(&program.sfz_path) else {
            continue;
        };
        if scan.weight.all_samples_missing() {
            all_samples_missing.insert(key.clone());
        }
        included.extend(
            scan.includes
                .iter()
                .filter_map(|path| crate::lexical_absolute(path).ok())
                .map(|path| canonical_key(&path))
                .filter(|include| include != key),
        );
    }
    programs.retain(|key, _| !included.contains(key) && !all_samples_missing.contains(key));
}

fn root_strings(roots: Vec<PathBuf>) -> Vec<String> {
    let mut dirs = roots
        .into_iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.dedup_by(|left, right| canonical_key(Path::new(left)) == canonical_key(Path::new(right)));
    dirs
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

/// Canonical `.sfz` and `.ariax` files under one root.
#[derive(Default)]
struct PatchFiles {
    sfz: Vec<PathBuf>,
    ariax: Vec<PathBuf>,
}

fn collect_patch_files(root: &Path, notices: &mut Vec<String>) -> PatchFiles {
    fn visit(dir: &Path, files: &mut PatchFiles, notices: &mut Vec<String>) {
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
                continue;
            }
            let list = if is_sfz(&path) {
                &mut files.sfz
            } else if ariax::is_ariax(&path) {
                &mut files.ariax
            } else {
                continue;
            };
            if let Ok(canonical) = crate::lexical_absolute(&path) {
                list.push(canonical);
            }
        }
    }
    let mut files = PatchFiles::default();
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

/// `..` が残ったパスは、root の外へ出うるので配下とみなさない。
pub(super) fn path_is_within(path: &Path, root: &Path) -> bool {
    strip_prefix_portable(path, root).is_some_and(|relative| {
        relative
            .components()
            .all(|component| !matches!(component, std::path::Component::ParentDir))
    })
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
