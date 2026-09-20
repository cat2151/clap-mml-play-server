//! TONE3000 の factory preset（`.t3kpreset`）を plugin state に組み立てる。
//!
//! preset も state も `T3KB` magic + JUCE [`ValueTree`](crate::juce_value_tree) で、
//! 木の形だけが違う:
//!
//! ```text
//! preset: T3KPreset{schemaVersion, name} → ChainSnapshot{…}, Params{Param{id, value}*}
//! state : TONE3000State{…, activePresetId, activePresetName}
//!         → PARAMETERS{PARAM{id, value}*}, MidiMappings, ChainSnapshot{…}
//! ```
//!
//! plugin が生成直後に保存した state を template にして、`ChainSnapshot` を preset の
//! ものへ差し替え、`PARAMETERS` の値を preset の `Params` で上書きし、
//! `activePresetId` / `activePresetName` を書く（[`tone3000_state_blob`]）。
//! preset の名前だけを書いても plugin は preset を読まない。

use anyhow::{bail, Context, Result};

use crate::juce_value_tree::{decode, encode, ValueTree, Var};

/// TONE3000 の CLAP plugin ID。
pub const TONE3000_PLUGIN_ID: &str = "com.tone3000.plugin";

const MAGIC: &[u8; 4] = b"T3KB";
const PRESET_TYPE: &str = "T3KPreset";
const STATE_TYPE: &str = "TONE3000State";
const CHAIN_SNAPSHOT: &str = "ChainSnapshot";

/// 読み込んだ `.t3kpreset`。
#[derive(Clone, Debug, PartialEq)]
pub struct Tone3000Preset {
    /// preset 内の `name`。catalog はこれを表示名・JSON の値にする。
    pub name: String,
    pub tree: ValueTree,
}

/// `.t3kpreset` のバイト列を読む。
pub fn parse_t3k_preset(bytes: &[u8]) -> Result<Tone3000Preset> {
    let tree = decode(strip_magic(bytes)?).context("TONE3000 preset の ValueTree が読めない")?;
    if tree.type_name != PRESET_TYPE {
        bail!(
            "TONE3000 preset の root が {PRESET_TYPE} ではない: '{}'",
            tree.type_name
        );
    }
    let name = tree
        .property_string("name")
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow::anyhow!("TONE3000 preset に name が無い"))?
        .to_string();
    Ok(Tone3000Preset { name, tree })
}

/// plugin state（`T3KB` + ValueTree）を木にする。
pub fn parse_state(bytes: &[u8]) -> Result<ValueTree> {
    let tree = decode(strip_magic(bytes)?).context("TONE3000 state の ValueTree が読めない")?;
    if tree.type_name != STATE_TYPE {
        bail!(
            "TONE3000 state の root が {STATE_TYPE} ではない: '{}'",
            tree.type_name
        );
    }
    Ok(tree)
}

fn strip_magic(bytes: &[u8]) -> Result<&[u8]> {
    match bytes.split_at_checked(4) {
        Some((magic, rest)) if magic == MAGIC => Ok(rest),
        _ => bail!("TONE3000 の magic が T3KB ではない"),
    }
}

/// init state を template に、preset を載せた state を組む。
///
/// `preset_id` は plugin が `activePresetId` に持つ識別子（factory preset ではファイル名の uuid）。
pub fn tone3000_state_blob(
    init_state: &[u8],
    preset: &Tone3000Preset,
    preset_id: &str,
) -> Result<Vec<u8>> {
    let mut state = parse_state(init_state)?;
    let chain = preset
        .tree
        .child(CHAIN_SNAPSHOT)
        .ok_or_else(|| anyhow::anyhow!("preset '{}' に ChainSnapshot が無い", preset.name))?
        .clone();
    match state.child_mut(CHAIN_SNAPSHOT) {
        Some(slot) => *slot = chain,
        None => state.children.push(chain),
    }
    overwrite_parameters(&mut state, &preset.tree)?;
    state.set_property("activePresetId", Var::String(preset_id.to_string()));
    state.set_property("activePresetName", Var::String(preset.name.clone()));
    let mut bytes = MAGIC.to_vec();
    bytes.extend(encode(&state));
    Ok(bytes)
}

/// state の `PARAMETERS/PARAM[id]` を preset の `Params/Param[id]` の値で上書きする。
fn overwrite_parameters(state: &mut ValueTree, preset: &ValueTree) -> Result<()> {
    let Some(params) = preset.child("Params") else {
        return Ok(());
    };
    let parameters = state
        .child_mut("PARAMETERS")
        .ok_or_else(|| anyhow::anyhow!("TONE3000 state に PARAMETERS が無い"))?;
    for param in &params.children {
        let Some(id) = param.property_string("id") else {
            continue;
        };
        let Some(value) = param.property("value") else {
            continue;
        };
        match parameters
            .children
            .iter_mut()
            .find(|slot| slot.property_string("id") == Some(id))
        {
            Some(slot) => slot.set_property("value", value.clone()),
            None => parameters.children.push(param.clone()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
