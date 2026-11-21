use std::path::Path;

pub(super) fn roots(bundle: &str) -> crate::PatchCatalogResolution {
    let dirs = plugin_presets::apricot::apricot_preset_dirs();
    let mut resolution = super::patch_catalog::resolve_plain_directories(Some(&dirs));
    // Factory は bundle 以下の仮想 directory。resolved_patches を持つので通常 walk はしない。
    if Path::new(bundle).is_file() {
        resolution.dirs.push(bundle.into());
    }
    let mut bases = dirs;
    bases.push(bundle.into());
    resolution.base = crate::PatchBase::per_root(&bases);
    resolution
}

pub(super) fn resolve(bundle: &str) -> crate::PatchCatalogResolution {
    let mut resolution = roots(bundle);
    let mut patches = match factory_patches(Path::new(bundle)) {
        Ok(patches) => patches,
        Err(error) => {
            resolution
                .notices
                .push(format!("Apricot factory: {error:#}"));
            Vec::new()
        }
    };
    for root in resolution.dirs.iter().filter(|p| Path::new(p).is_dir()) {
        match plugin_presets::patch_list::collect_patches(root, |s| resolution.notices.push(s)) {
            Ok(user) => patches.extend(user.into_iter().filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("synthpatch"))
            })),
            Err(error) => resolution.notices.push(format!("Apricot user: {error:#}")),
        }
    }
    if patches.is_empty() {
        resolution.source_error = Some("Apricot の音色を解決できない".into());
    }
    patches.sort();
    patches.dedup();
    resolution.resolved_patches = Some(patches);
    resolution
}

mod discovery;
use discovery::factory_patches;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "実 Apricot CLAP が必要"]
    fn installed_factory_discovery() {
        let bundle = std::env::var("CMRT_TEST_APRICOT_CLAP").unwrap();
        let patches = factory_patches(Path::new(&bundle)).unwrap();
        eprintln!(
            "Apricot: {} patches; first={:?}",
            patches.len(),
            patches.first()
        );
        assert!(patches.len() > 80);
        let source = resolve(&bundle);
        let startup = roots(&bundle);
        assert_eq!(source.base, startup.base);
        assert!(source.source_error.is_none());
        assert!(!source.dirs.is_empty());
        for patch in source.resolved_patches.unwrap() {
            let display = source.base.display(&patch);
            assert_eq!(Path::new(&startup.base.resolve(&display)), patch);
        }
    }
}
