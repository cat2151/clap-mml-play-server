//! OB-Xf の音色置き場と FXP → CLAP state の変換。
//!
//! 根拠: surge-synthesizer/OB-Xf e03452fbaf2dde6b73df78ea1a35bcb562f7d184 の
//! Utils::{getPatchFolderFor,getDocumentFolder} と sst-plugininfra の paths_windows.cpp。
//! preset-discovery と同じ system factory / local factory / user の 3 か所を列挙する。
//! StateManager::{getProgramStateInformation,getPluginStateInformation,setPluginStateInformation}
//! の program/state の違いに合わせて包む（v1.0.0 でも同形式）。

pub const OBXF_PLUGIN_ID: &str = "org.surge-synth-team.OB-Xf";
pub const OBXF_PATCH_TAG: &str = "o";

/// FXP の program XML を plugin state の `<program>` に包む。
/// パラメータ・VoiceMatrix・音色情報と streaming version はそのまま残す。
pub fn obxf_state_blob(fxp: &[u8]) -> anyhow::Result<Vec<u8>> {
    use crate::surge_fx::juce_xml;
    use anyhow::{ensure, Context};
    use xmltree::{Element, XMLNode};

    let header = fxp.get(..60).context("OB-Xf FXP header が短い")?;
    ensure!(
        &header[..4] == b"CcnK" && &header[8..12] == b"FPCh" && &header[16..20] == b"OBXf",
        "OB-Xf FXP の magic / ID が不正"
    );
    let length = u32::from_be_bytes(header[56..60].try_into()?) as usize;
    ensure!(fxp.len() - 60 == length, "OB-Xf FXP の chunk 長が不一致");
    let xml = juce_xml::decode(&fxp[60..])?;
    // 一部 factory FXP は JUCE の XML 長に末尾の NUL も含めている。
    let mut program = Element::parse(xml.trim_end_matches('\0').as_bytes())
        .context("OB-Xf program XML が不正")?;
    ensure!(
        program.name == "OB-Xf" && program.get_child("program").is_none(),
        "OB-Xf program XML ではない"
    );
    let version = program
        .attributes
        .get("ob-xf_version")
        .context("OB-Xf program に streaming version が無い")?
        .clone();
    program.name = "program".into();
    let mut state = Element::new("OB-Xf");
    state.attributes.insert("ob-xf_version".into(), version);
    state
        .attributes
        .insert("single-program-format".into(), "1".into());
    state.children.push(XMLNode::Element(program));
    let mut out = Vec::new();
    state.write(&mut out)?;
    Ok(juce_xml::encode(std::str::from_utf8(&out)?))
}

pub fn is_obxf(plugin_id: Option<&str>, plugin_path: &str) -> bool {
    match plugin_id {
        Some(id) => id == OBXF_PLUGIN_ID,
        None => std::path::Path::new(plugin_path)
            .file_stem()
            .is_some_and(|stem| stem.eq_ignore_ascii_case("OB-Xf")),
    }
}

#[cfg(windows)]
pub fn obxf_preset_dirs() -> Vec<String> {
    preset_dirs(
        std::env::var_os("PROGRAMDATA").map(std::path::PathBuf::from),
        dirs::data_local_dir(),
        std::env::var_os("OBXF_DOCUMENT_FOLDER")
            .map(std::path::PathBuf::from)
            .or_else(dirs::document_dir),
    )
}

#[cfg(not(windows))]
pub fn obxf_preset_dirs() -> Vec<String> {
    Vec::new()
}

#[cfg(any(windows, test))]
fn preset_dirs(
    system: Option<std::path::PathBuf>,
    local: Option<std::path::PathBuf>,
    documents: Option<std::path::PathBuf>,
) -> Vec<String> {
    [system, local, documents]
        .into_iter()
        .flatten()
        .map(|root| {
            root.join("Surge Synth Team")
                .join("OB-Xf")
                .join("Patches")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_match_the_plugins_three_locations() {
        let roots = preset_dirs(
            Some("X:/shared".into()),
            Some("X:/local".into()),
            Some("X:/documents-override".into()),
        );
        let roots: Vec<_> = roots.iter().map(|p| p.replace('\\', "/")).collect();
        assert_eq!(
            roots,
            [
                "X:/shared/Surge Synth Team/OB-Xf/Patches",
                "X:/local/Surge Synth Team/OB-Xf/Patches",
                "X:/documents-override/Surge Synth Team/OB-Xf/Patches",
            ]
        );
    }

    #[test]
    fn explicit_identity_takes_precedence_over_filename() {
        assert!(is_obxf(Some(OBXF_PLUGIN_ID), "elsewhere.clap"));
        assert!(is_obxf(None, "X:/plugins/OB-Xf.clap"));
        assert!(!is_obxf(Some("other"), "OB-Xf.clap"));
    }
}

#[cfg(test)]
mod state_tests;
