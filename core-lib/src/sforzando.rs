//! sforzando の `.sfz` 音色。
//!
//! 任意の `.sfz` は Sforzando 固有の CEGP/AriaSave state adapter でロードする。
//! factory preset 用の `clap.preset-load/2` とは別経路である。

mod codec;

use std::io::Cursor;

use anyhow::Context;
use xmltree::{Element, EmitterConfig, XMLNode};

pub use cmrt_server_config::SFORZANDO_PLUGIN_ID;
pub(crate) use cmrt_server_config::{resolve_sforzando_program, SforzandoProgramRef};

const SFZ_EXTENSION: &str = ".sfz";
const PATH_SEPARATORS: [char; 2] = ['/', '\\'];

/// patch path が `.sfz` を指しているか。拡張子の大小文字は区別しない。
pub fn is_sfz_patch_path(patch: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(|component| {
        component.len() > SFZ_EXTENSION.len()
            && component
                .get(component.len() - SFZ_EXTENSION.len()..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(SFZ_EXTENSION))
    })
}

/// Instance creation immediately saved this template. Preserve every unrelated setting and
/// replace only the first AriaSave/Slot program coordinate.
pub(crate) fn sforzando_state_blob(
    init_state: &[u8],
    program: &SforzandoProgramRef,
) -> anyhow::Result<Vec<u8>> {
    let xml = codec::decode(init_state).context("Sforzando CEGP init state を decode できない")?;
    let mut root =
        Element::parse(Cursor::new(xml.as_bytes())).context("Sforzando AriaSave XML が不正")?;
    if root.name != "AriaSave" {
        anyhow::bail!(
            "Sforzando state root element が AriaSave ではない: '{}'",
            root.name
        );
    }
    if find_program_slot(&mut root).is_none() {
        let insert_at = root
            .children
            .iter()
            .position(
                |node| matches!(node, XMLNode::Element(element) if element.name == "EffectSlot"),
            )
            .unwrap_or(root.children.len());
        root.children
            .insert(insert_at, XMLNode::Element(empty_program_slot()));
    }
    let slot = find_program_slot(&mut root).expect("checked above");
    slot.attributes
        .insert("name".to_string(), program.program_name.clone());
    slot.attributes
        .insert("bankId".to_string(), program.bank_id.clone());
    slot.attributes
        .insert("version".to_string(), program.bank_version.clone());

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
