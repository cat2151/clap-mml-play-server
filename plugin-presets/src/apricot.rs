//! Apricot の内蔵 factory 音色と user の `.synthpatch`。
use std::path::{Component, Path};

pub const APRICOT_PLUGIN_ID: &str = "nakst.Apricot";

pub fn is_apricot(id: Option<&str>, path: &str) -> bool {
    id.map_or_else(
        || {
            Path::new(path)
                .file_stem()
                .is_some_and(|s| s.eq_ignore_ascii_case("Apricot"))
        },
        |id| id == APRICOT_PLUGIN_ID,
    )
}

pub fn apricot_preset_dirs() -> Vec<String> {
    dirs::document_dir()
        .map(|p| {
            p.join("Apricot Synthesizer/Patches")
                .to_string_lossy()
                .into_owned()
        })
        .into_iter()
        .collect()
}

/// bundle 以下の仮想パスを、実 bundle と CLAP の load key へ戻す。
pub fn factory_patch(path: &Path) -> Option<(&Path, String)> {
    let bundle = path.ancestors().skip(1).find(|p| {
        p.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("clap"))
    })?;
    let relative = path.strip_prefix(bundle).ok()?;
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let key = relative.to_str()?.replace('\\', "/");
    valid_factory_key(&key).then_some((bundle, key))
}

pub fn valid_factory_key(key: &str) -> bool {
    key.starts_with("patches/")
        && key.ends_with(".synthpatch")
        && !key.contains([':', '\\'])
        && key
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_factory_reference_retains_the_exact_load_key() {
        let path =
            Path::new("X:/plugins/Apricot.clap/patches/Factory Pads/PD Elder's Wisdom.synthpatch");
        let (bundle, key) = factory_patch(path).unwrap();
        assert_eq!(bundle, Path::new("X:/plugins/Apricot.clap"));
        assert_eq!(key, "patches/Factory Pads/PD Elder's Wisdom.synthpatch");
        assert!(factory_patch(Path::new("X:/user/Pads/Voice.synthpatch")).is_none());
        for key in [
            "patches/../escape.synthpatch",
            "patches//empty.synthpatch",
            "patches/X:/bad.synthpatch",
            "other/Voice.synthpatch",
            "patches/Voice.fxp",
        ] {
            assert!(!valid_factory_key(key), "{key}");
        }
    }

    #[test]
    fn explicit_plugin_identity_takes_precedence_over_filename() {
        assert!(is_apricot(Some(APRICOT_PLUGIN_ID), "X:/renamed.clap"));
        assert!(is_apricot(None, "X:/Apricot.clap"));
        assert!(!is_apricot(Some("other"), "X:/Apricot.clap"));
    }
}
