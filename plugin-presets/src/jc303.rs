//! JC303（midilab）の preset（`.json`）を plugin state に組み立てる。
//!
//! preset は `values` に parameter ID → 値を持つ。値はパラメータ自身の単位で、choice は選択肢名、
//! switch は bool で書かれる。plugin state は JUCE APVTS の XML で、`<PARAM id=… value=…/>` の value も
//! 同じ単位なので、init state を雛形に書き込む（[`jc303_state_blob`]）。preset に無い parameter は
//! init state の既定値のまま残る。本体の preset 読み込みも、preset に無い parameter を既定値へ戻す。
//!
//! choice の名前を番号へ直す表は state に無いので、呼び出し側が plugin へ問い合わせて渡す。

use std::io::Cursor;

use anyhow::{bail, Context, Result};
use serde_json::Value;
use xmltree::{Element, EmitterConfig};

use crate::surge_fx::juce_xml;

pub const JSON_EXTENSION: &str = "json";

/// preset に `modKnob1` / `modKnob2` が無いときの割り当て。本体の既定と同じ。
const DEFAULT_MOD_KNOBS: [&str; 2] = ["filterMorph", "filterFm"];

pub fn is_jc303_preset_path(patch: &str) -> bool {
    std::path::Path::new(patch)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(JSON_EXTENSION))
}

/// 本体が preset を置く `Documents\midilab\JC303\presets`。直下の folder 1 つが bank 1 つ
/// （`factory` は本体が初回起動時に書き出す）。
pub fn jc303_preset_dirs() -> Vec<String> {
    dirs::document_dir()
        .map(|documents| {
            documents
                .join("midilab")
                .join("JC303")
                .join("presets")
                .to_string_lossy()
                .into_owned()
        })
        .into_iter()
        .collect()
}

/// init state を雛形に、preset の値と MOD knob の割り当てを書き込んだ state を組む。
///
/// `choice_index(parameter ID, 選択肢名)` は選択肢の番号を返す。雛形に無い parameter が
/// preset にあれば、plugin の版が違うとみなしてエラーにする。
pub fn jc303_state_blob(
    init_state: &[u8],
    preset_json: &str,
    mut choice_index: impl FnMut(&str, &str) -> Result<usize>,
) -> Result<Vec<u8>> {
    let preset: Value = serde_json::from_str(preset_json).context("JC303 preset の JSON が不正")?;
    let preset = preset
        .as_object()
        .context("JC303 preset の root が object ではない")?;
    let mut state = Element::parse(Cursor::new(juce_xml::decode(init_state)?.as_bytes()))
        .context("JC303 の init state の XML が不正")?;

    if let Some(values) = preset.get("values") {
        let values = values
            .as_object()
            .context("JC303 preset の values が object ではない")?;
        for (id, value) in values {
            let plain = match value {
                Value::Bool(on) => f64::from(u8::from(*on)),
                Value::Number(number) => number
                    .as_f64()
                    .with_context(|| format!("JC303 preset の '{id}' が数として読めない"))?,
                Value::String(choice) => choice_index(id, choice)? as f64,
                other => bail!("JC303 preset の '{id}' の値の型が読めない: {other}"),
            };
            let param = state
                .children
                .iter_mut()
                .filter_map(|node| node.as_mut_element())
                .find(|element| {
                    element.name == "PARAM"
                        && element.attributes.get("id").map(String::as_str) == Some(id)
                })
                .with_context(|| {
                    format!("JC303 の state に parameter '{id}' が無い（preset にはある）")
                })?;
            param
                .attributes
                .insert("value".to_string(), plain.to_string());
        }
    }

    for (index, default) in DEFAULT_MOD_KNOBS.iter().enumerate() {
        let key = format!("modKnob{}", index + 1);
        let knob = match preset.get(&key) {
            None => default,
            Some(value) => value
                .as_str()
                .with_context(|| format!("JC303 preset の {key} が文字列ではない"))?,
        };
        state.attributes.insert(key, knob.to_string());
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

#[cfg(test)]
mod tests;
