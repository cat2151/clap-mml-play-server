//! JC303 の音色切替（`.json` の値を init state へ書き込んで CLAP state として流し込む）。
//!
//! 組み立ては [`crate::jc303_preset::jc303_state_blob`]。ここは choice の名前を番号へ直す問い合わせと、
//! 組んだ state のロードだけを持つ。
//!
//! JUCE の CLAP param id は parameter ID 文字列の `String::hashCode()`（[`juce_param_id`]）。
//! CLAP の値は 0..1 に正規化され、choice も stepped として公開されないので、選択肢の数は info から
//! 取れない。そこで 0..1 を細かく刻んで `value_to_text` を読み、並んだ選択肢名から番号を決める。

use anyhow::{Context, Result};
use clack_extensions::params::PluginParams;
use clack_host::prelude::{ClapId, PluginInstance};

use super::patch_state::load_plugin_state;
use super::RealtimeRenderer;
use crate::host::MidiRenderHost;
use crate::jc303_preset::jc303_state_blob;
use cmrt_server_config::JC303_PLUGIN_ID;

/// 選択肢を読み出すときの 0..1 の刻み数。選択肢が数百あっても取りこぼさない細かさ。
const CHOICE_SCAN_STEPS: u32 = 1024;

impl RealtimeRenderer {
    /// `.json` の音色を選ぶ。載っているプラグインが JC303 でなければ送らずにエラーにする。
    pub(super) fn load_jc303_patch(&mut self, patch_path: &str) -> Result<()> {
        let init_state = self
            .init_state
            .take()
            .context("初期 state が未取得のため JC303 の音色を組めない")?;
        let plugin_id = self.plugin_id.clone();
        let result = load_jc303_state(
            self.plugin_instance_mut(),
            &plugin_id,
            &init_state,
            patch_path,
        );
        self.init_state = Some(init_state);
        result
    }
}

/// `.json` を読み、`init_state` を雛形に組んだ state を流し込む。`activate()` 前にも呼べるよう
/// 自由関数にしてある。
pub(super) fn load_jc303_state(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    plugin_id: &str,
    init_state: &[u8],
    patch_path: &str,
) -> Result<()> {
    ensure_jc303_capable(plugin_id)?;
    let json = std::fs::read_to_string(patch_path)
        .with_context(|| format!("JC303 の音色ファイルを読めない '{patch_path}'"))?;
    let state = jc303_state_blob(init_state, &json, |id, name| {
        choice_index(plugin_instance, id, name)
    })
    .with_context(|| format!("JC303 の音色を組めない '{patch_path}'"))?;
    load_plugin_state(plugin_instance, &state)
        .with_context(|| format!("JC303 の音色のロードに失敗 '{patch_path}'"))
}

/// parameter `id` の選択肢のうち、`name` が何番目か。
fn choice_index(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    id: &str,
    name: &str,
) -> Result<usize> {
    let handle = plugin_instance.plugin_handle();
    let params = handle
        .get_extension::<PluginParams>()
        .context("JC303 が clap.params を持たない")?;
    let param_id = juce_param_id(id);
    let mut choices: Vec<String> = Vec::new();
    for step in 0..=CHOICE_SCAN_STEPS {
        let mut buffer = [0u8; 256];
        let text = params
            .value_to_text(
                &handle,
                param_id,
                f64::from(step) / f64::from(CHOICE_SCAN_STEPS),
                &mut buffer,
            )
            .map_err(|_| anyhow::anyhow!("JC303 の parameter '{id}' の表示名を読めない"))?;
        let text = String::from_utf8_lossy(text);
        if choices.last().map(String::as_str) != Some(text.as_ref()) {
            choices.push(text.into_owned());
        }
    }
    choices
        .iter()
        .position(|choice| choice == name)
        .with_context(|| format!("JC303 の '{id}' に選択肢 '{name}' が無い（選択肢: {choices:?}）"))
}

/// JUCE が CLAP param id にする、parameter ID 文字列の `String::hashCode()`。
pub(super) fn juce_param_id(id: &str) -> ClapId {
    let hash = id
        .chars()
        .fold(0u32, |hash, c| hash.wrapping_mul(31).wrapping_add(c as u32));
    ClapId::new(hash)
}

/// `.json` を受け付けられるプラグインが載っているか。
pub(super) fn ensure_jc303_capable(plugin_id: &str) -> Result<()> {
    if plugin_id == JC303_PLUGIN_ID {
        return Ok(());
    }
    anyhow::bail!(
        "'.json' の音色は plugin_id = '{JC303_PLUGIN_ID}' でしか読めない（いま載っているのは '{plugin_id}'）"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実機の JC303 が公開している CLAP param id と一致すること。
    #[test]
    fn param_ids_are_the_juce_string_hash() {
        assert_eq!(juce_param_id("cutoff"), ClapId::new(2_945_903_597));
        assert_eq!(juce_param_id("lfoDestination"), ClapId::new(3_053_209_241));
        assert_eq!(juce_param_id("modSlot3Amount"), ClapId::new(1_198_944_843));
    }
}
