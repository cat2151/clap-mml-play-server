//! Dragonfly Reverb（Hall / Room / Plate / Early Reflections）の組み込み preset を plugin state に組み立てる。
//!
//! preset は plugin 本体に数値表として組み込まれていて、ファイルも CLAP の preset 拡張も無い。
//! 表は [`tables`] に写してあり、DPF の state（`key\0value\0` の並び、`\xfe` で終端）の
//! parameter 欄を preset の値で上書きし、`preset` state に名前を書く（[`dragonfly_state_blob`]）。
//! `preset` state だけを書いても plugin は decay しか変えない（数値の適用は GUI 側の仕事）。

use anyhow::{bail, Result};

mod tables;

/// 組み込み preset を持つ plugin 1 つ。
#[derive(Debug)]
pub struct DragonflyPlugin {
    pub plugin_id: &'static str,
    pub name: &'static str,
    /// インストール先の `<file_stem>.clap`。
    pub file_stem: &'static str,
    /// DPF state に `preset` キーを持つか（Early Reflections は持たない）。
    pub has_preset_state: bool,
    /// preset が値を持つ parameter の symbol。
    pub symbols: &'static [&'static str],
    pub presets: &'static [DragonflyPreset],
}

#[derive(Debug)]
pub struct DragonflyPreset {
    pub name: &'static str,
    /// [`DragonflyPlugin::symbols`] と同じ順。
    pub values: &'static [f32],
}

pub const DRAGONFLY_PLUGINS: [DragonflyPlugin; 4] = [
    tables::HALL,
    tables::ROOM,
    tables::PLATE,
    tables::EARLY_REFLECTIONS,
];

/// plugin ID が Dragonfly Reverb のどれかなら、その表を返す。
pub fn dragonfly_plugin(plugin_id: &str) -> Option<&'static DragonflyPlugin> {
    DRAGONFLY_PLUGINS
        .iter()
        .find(|plugin| plugin.plugin_id == plugin_id)
}

impl DragonflyPlugin {
    pub fn preset(&self, name: &str) -> Result<&'static DragonflyPreset> {
        match self.presets.iter().find(|preset| preset.name == name) {
            Some(preset) => Ok(preset),
            None => bail!("{} に preset '{name}' は無い", self.name),
        }
    }
}

const STATE_BEGIN: &str = "__dpf_state_begin__";
const STATE_END: &str = "__dpf_state_end__";
const PARAMETERS_BEGIN: &str = "__dpf_parameters_begin__";
const PARAMETERS_END: &str = "__dpf_parameters_end__";
const PRESET_STATE_KEY: &str = "preset";
const TERMINATOR: u8 = 0xfe;

/// init state を template に、preset を載せた state を組む。
///
/// template に無い symbol が表にあればエラー（plugin の版が表と違う。黙って一部だけ載せない）。
pub fn dragonfly_state_blob(
    init_state: &[u8],
    plugin: &DragonflyPlugin,
    preset: &DragonflyPreset,
) -> Result<Vec<u8>> {
    let mut tokens = parse_state(init_state)?;
    let mut section: Option<&'static str> = None;
    let mut wrote_preset_state = false;
    let mut written = vec![false; plugin.symbols.len()];
    let mut index = 0;
    while index < tokens.len() {
        if let Some(marker) = [STATE_BEGIN, STATE_END, PARAMETERS_BEGIN, PARAMETERS_END]
            .into_iter()
            .find(|marker| tokens[index] == *marker)
        {
            section = Some(marker);
            index += 1;
            continue;
        }
        if index + 1 >= tokens.len() {
            bail!("{} の state で '{}' に値が無い", plugin.name, tokens[index]);
        }
        let key = tokens[index].clone();
        if section == Some(STATE_BEGIN) && key == PRESET_STATE_KEY {
            tokens[index + 1] = preset.name.to_string();
            wrote_preset_state = true;
        } else if section == Some(PARAMETERS_BEGIN) {
            if let Some(position) = plugin.symbols.iter().position(|symbol| *symbol == key) {
                tokens[index + 1] = preset.values[position].to_string();
                written[position] = true;
            }
        }
        index += 2;
    }
    if plugin.has_preset_state && !wrote_preset_state {
        bail!("{} の state に '{PRESET_STATE_KEY}' が無い", plugin.name);
    }
    if let Some(position) = written.iter().position(|written| !written) {
        bail!(
            "{} の state に parameter '{}' が無い",
            plugin.name,
            plugin.symbols[position]
        );
    }
    Ok(encode_state(&tokens))
}

/// DPF state を `\0` 区切りの語の列にする。
pub fn parse_state(bytes: &[u8]) -> Result<Vec<String>> {
    let Some(end) = bytes.iter().position(|byte| *byte == TERMINATOR) else {
        bail!("Dragonfly の state に終端 0xfe が無い");
    };
    let body = &bytes[..end];
    let Some(body) = body.strip_suffix(&[0]) else {
        bail!("Dragonfly の state が \\0 で終わっていない");
    };
    body.split(|byte| *byte == 0)
        .map(|token| {
            String::from_utf8(token.to_vec())
                .map_err(|_| anyhow::anyhow!("Dragonfly の state が UTF-8 でない"))
        })
        .collect()
}

fn encode_state(tokens: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for token in tokens {
        bytes.extend_from_slice(token.as_bytes());
        bytes.push(0);
    }
    bytes.push(TERMINATOR);
    bytes.push(0);
    bytes
}

#[cfg(test)]
mod tests;
