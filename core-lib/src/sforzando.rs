//! sforzando の音色（`.sfz` と `.ariax`）。
//!
//! 任意の `.sfz` と ARIA の preset `.ariax` は、Sforzando 固有の CEGP/AriaSave state adapter で
//! ロードする。factory preset 用の `clap.preset-load/2` とは別経路である。

pub(crate) mod codec;

use std::io::Cursor;

use anyhow::Context;
use xmltree::{Element, EmitterConfig, XMLNode};

pub use cmrt_server_config::SFORZANDO_PLUGIN_ID;
pub(crate) use cmrt_server_config::{
    resolve_sforzando_preset, resolve_sforzando_program, SforzandoPresetRef, SforzandoProgramRef,
};

const SFZ_EXTENSION: &str = ".sfz";
const ARIAX_EXTENSION: &str = ".ariax";
const PATH_SEPARATORS: [char; 2] = ['/', '\\'];

/// patch path が `.sfz` を指しているか。拡張子の大小文字は区別しない。
pub fn is_sfz_patch_path(patch: &str) -> bool {
    has_component_with_extension(patch, SFZ_EXTENSION)
}

/// patch path が `.ariax`（ARIA の preset）を指しているか。拡張子の大小文字は区別しない。
pub fn is_ariax_patch_path(patch: &str) -> bool {
    has_component_with_extension(patch, ARIAX_EXTENSION)
}

/// Sforzando が読む形（`.sfz` か `.ariax`）の patch path か。
pub fn is_sforzando_patch_path(patch: &str) -> bool {
    is_sfz_patch_path(patch) || is_ariax_patch_path(patch)
}

fn has_component_with_extension(patch: &str, extension: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(|component| {
        component.len() > extension.len()
            && component
                .get(component.len() - extension.len()..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(extension))
    })
}

/// ARIA engine がサンプルを disk から読む方式。`AriaSave/Settings@streaming` に対応する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SfzStreaming {
    /// init state の値のまま。load 時は各サンプルの頭だけを読み、残りは発音中に読む。
    /// 1 倍速の realtime 演奏なら読み出しが間に合う。
    PluginDefault,
    /// `streaming="0"`。load 時に各サンプルを最後まで読む。faster-than-realtime の offline
    /// render は、発音中の読み出しが追いつかないと voice が途中で止まるので、こちらを使う。
    Disabled,
}

/// Instance creation immediately saved this template. Preserve every unrelated setting and
/// replace only the first AriaSave/Slot program coordinate (and `Settings@streaming` when
/// `streaming` is [`SfzStreaming::Disabled`]).
pub(crate) fn sforzando_state_blob(
    init_state: &[u8],
    program: &SforzandoProgramRef,
    streaming: SfzStreaming,
) -> anyhow::Result<Vec<u8>> {
    let mut root = parse_template(init_state)?;
    ensure_program_slot(&mut root);
    apply_streaming(&mut root, streaming)?;
    let slot = find_program_slot(&mut root).expect("ensured above");
    slot.attributes
        .insert("name".to_string(), program.program_name.clone());
    slot.attributes
        .insert("bankId".to_string(), program.bank_id.clone());
    slot.attributes
        .insert("version".to_string(), program.bank_version.clone());
    encode_root(&root)
}

/// `.ariax`（ARIA の preset。平文の AriaSave XML）の `Slot` 要素を、init state template の
/// `Slot` と丸ごと差し替えた state を作る。`Slot` 以外（`Settings` / `EffectSlot` / `GUI`）は
/// template のものを使い、`.ariax` 側の `Settings` は読まない。`streaming` の扱いは
/// [`sforzando_state_blob`] と同じ。
pub(crate) fn sforzando_ariax_state_blob(
    init_state: &[u8],
    ariax_xml: &str,
    streaming: SfzStreaming,
) -> anyhow::Result<Vec<u8>> {
    let mut preset = Element::parse(Cursor::new(ariax_xml.as_bytes()))
        .context("`.ariax` の AriaSave XML が不正")?;
    if preset.name != "AriaSave" {
        anyhow::bail!(
            "`.ariax` の root element が AriaSave ではない: '{}'",
            preset.name
        );
    }
    let preset_slot = find_program_slot(&mut preset)
        .ok_or_else(|| anyhow::anyhow!("`.ariax` に Slot id=\"0\" が無い"))?
        .clone();

    let mut root = parse_template(init_state)?;
    ensure_program_slot(&mut root);
    apply_streaming(&mut root, streaming)?;
    *find_program_slot(&mut root).expect("ensured above") = preset_slot;
    encode_root(&root)
}

fn parse_template(init_state: &[u8]) -> anyhow::Result<Element> {
    let xml = codec::decode(init_state).context("Sforzando CEGP init state を decode できない")?;
    let root =
        Element::parse(Cursor::new(xml.as_bytes())).context("Sforzando AriaSave XML が不正")?;
    if root.name != "AriaSave" {
        anyhow::bail!(
            "Sforzando state root element が AriaSave ではない: '{}'",
            root.name
        );
    }
    Ok(root)
}

fn ensure_program_slot(root: &mut Element) {
    if find_program_slot(root).is_some() {
        return;
    }
    let insert_at = root
        .children
        .iter()
        .position(|node| matches!(node, XMLNode::Element(element) if element.name == "EffectSlot"))
        .unwrap_or(root.children.len());
    root.children
        .insert(insert_at, XMLNode::Element(empty_program_slot()));
}

fn apply_streaming(root: &mut Element, streaming: SfzStreaming) -> anyhow::Result<()> {
    if streaming == SfzStreaming::Disabled {
        let settings = root.get_mut_child("Settings").ok_or_else(|| {
            anyhow::anyhow!("Sforzando init state に Settings が無く、streaming を無効にできない")
        })?;
        settings
            .attributes
            .insert("streaming".to_string(), "0".to_string());
    }
    Ok(())
}

fn encode_root(root: &Element) -> anyhow::Result<Vec<u8>> {
    let mut output = Vec::new();
    root.write_with_config(
        &mut output,
        EmitterConfig::new()
            .write_document_declaration(true)
            .perform_indent(true),
    )
    .context("Sforzando AriaSave XML を encode できない")?;
    codec::encode(&output)
}

fn find_program_slot(element: &mut Element) -> Option<&mut Element> {
    if element.name == "Slot" && element.attributes.get("id").is_none_or(|id| id == "0") {
        return Some(element);
    }
    element.children.iter_mut().find_map(|node| match node {
        XMLNode::Element(child) => find_program_slot(child),
        _ => None,
    })
}

fn empty_program_slot() -> Element {
    let mut slot = Element::new("Slot");
    for (name, value) in [
        ("id", "0"),
        ("channel", "-1"),
        ("poly", "32"),
        ("tuning", "0"),
        ("pb_range", "-1"),
        ("ptrans", "0"),
        ("mtrans", "0"),
        ("moctave", "0"),
        ("sc", "1"),
        ("mute", "0"),
    ] {
        slot.attributes.insert(name.to_string(), value.to_string());
    }
    let mut main = Element::new("Main");
    main.attributes.insert("id".to_string(), "0".to_string());
    main.attributes.insert("value".to_string(), "1".to_string());
    slot.children.push(XMLNode::Element(main));
    slot
}

#[cfg(test)]
mod tests;
