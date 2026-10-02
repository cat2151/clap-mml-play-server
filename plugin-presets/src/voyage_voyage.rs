//! Voyage Voyage（Musical Entropy のシマーリバーブ）の factory preset（`.pst`）を plugin state に組み立てる。
//!
//! `.pst` は `<VoyageVoyage …/>` 1 要素の XML で、属性が param ごとの値。plugin state は
//! 同じ要素を JUCE の [`juce_xml`] 形式に包んだ物で、`.pst` に無い属性（`TRUEST` など）を
//! 持つことがある。そこで init state を雛形にして `.pst` の属性で上書きする
//! （[`voyage_state_blob`]）。雛形に無い属性が `.pst` にあれば、plugin の版が違うとみなしてエラーにする。

use std::io::Cursor;
use std::path::Path;

use anyhow::{bail, Context, Result};
use xmltree::{Element, EmitterConfig};

use crate::effect_preset::{relative_display, PresetValue};
use crate::surge_fx::juce_xml;

pub const VOYAGE_VOYAGE_PLUGIN_ID: &str = "com.MusicalEntropy.VoyageVoyagex1";

pub const PST_EXTENSION: &str = "pst";

/// Voyage Voyage の preset の分類。どの folder の preset にもシマーが掛かるので kind は 1 つ。
pub const VOYAGE_VOYAGE_CATEGORY: &str = "Space / Imaging";
pub const VOYAGE_VOYAGE_KIND: &str = "Shimmer Reverb";

const ROOT_ELEMENT: &str = "VoyageVoyage";

/// `.pst` と plugin state に共通する `<VoyageVoyage …/>` を読む。
pub fn parse_voyage_xml(xml: &str) -> Result<Element> {
    let root =
        Element::parse(Cursor::new(xml.as_bytes())).context("Voyage Voyage の XML が不正")?;
    if root.name != ROOT_ELEMENT {
        bail!(
            "Voyage Voyage の root element が {ROOT_ELEMENT} ではない: '{}'",
            root.name
        );
    }
    Ok(root)
}

/// init state を雛形に、`.pst` の属性を上書きした state を組む。
pub fn voyage_state_blob(init_state: &[u8], pst_xml: &str) -> Result<Vec<u8>> {
    let mut state =
        parse_voyage_xml(&juce_xml::decode(init_state)?).context("Voyage Voyage の init state")?;
    let preset = parse_voyage_xml(pst_xml).context(".pst")?;
    for (name, value) in preset.attributes {
        match state.attributes.get_mut(&name) {
            Some(slot) => *slot = value,
            None => bail!("Voyage Voyage の state に属性 '{name}' が無い（.pst にはある）"),
        }
    }
    Ok(juce_xml::encode(&emit(&state)))
}

fn emit(root: &Element) -> String {
    let mut out = Vec::new();
    root.write_with_config(
        &mut out,
        EmitterConfig::new()
            .write_document_declaration(true)
            .perform_indent(false),
    )
    .expect("Vec<u8> への XML 書き出しは失敗しない");
    String::from_utf8(out).expect("xmltree は UTF-8 を書く")
}

/// Presets root からの相対パスから拡張子を除いた物を値と表示名にする。読めない `.pst` はエラー。
pub fn voyage_voyage_value(root: &Path, path: &Path, _plugin_name: &str) -> Result<PresetValue> {
    let xml = std::fs::read_to_string(path).context("読めない")?;
    parse_voyage_xml(&xml)?;
    let relative = relative_display(root, path);
    let value = relative
        .strip_suffix(&format!(".{PST_EXTENSION}"))
        .unwrap_or(&relative)
        .to_string();
    Ok(PresetValue {
        shown: value.clone(),
        value,
        category: VOYAGE_VOYAGE_CATEGORY.to_string(),
        kind: VOYAGE_VOYAGE_KIND.to_string(),
    })
}

#[cfg(test)]
mod tests;
