//! CLAP factory の descriptor 選択と、生成した instance の capability probe。
//!
//! プラグインごとの差（Surge XT は audio input 1 本 + CLAP note dialect、
//! Dexed は audio input 0 本 + MIDI dialect のみ）をここへ集める。レンダリングループは
//! [`PluginCapabilities`] だけを見て port と event を組み立てる。
//!
//! エラー文にはプラグインパスを含めない。ここからはパスが見えないため、
//! 呼び出し側（サーバーの `main`）が `plugin_path=...` を context として足すこと。

use anyhow::Result;
use clack_extensions::audio_ports::{AudioPortFlags, AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::note_ports::{NoteDialects, NotePortInfoBuffer, PluginNotePorts};
use clack_host::prelude::*;

use crate::host::MidiRenderHost;

/// レンダリングループが読み書きするチャンネル数（ステレオ固定）。
const STEREO_CHANNELS: u32 = 2;

/// factory から選ばれた 1 件の plugin descriptor。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedDescriptor {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
}

impl SelectedDescriptor {
    /// 起動ログ 1 行ぶんの表現。設定ミスの診断はこのログで行う。
    pub fn log_fields(&self) -> String {
        format!(
            "id={} name={} vendor={} version={}",
            self.id, self.name, self.vendor, self.version
        )
    }
}

/// instance 生成直後（`activate()` の前）に読んだプラグインの能力。
///
/// レンダリングループは main output port（port 0）1 本だけを読み、input も
/// 使うなら 1 本だけ渡す。したがって port の**本数**そのものは能力ではなく、
/// 「input を渡す必要があるか」と「port 0 が使える形か」だけを見る。
///
/// # port を 1 本しか渡さないことについて
/// Surge XT 1.3.4 は output port を 3 本広告する（実測）。CLAP の契約では host は
/// 広告された本数ぶんの buffer を渡すべきだが、このコードは以前から port 0 だけを
/// 渡して動いてきた。ここを直すと Surge の出音が変わりうるため、Phase 1 では
/// 現行動作を維持する。契約違反として残っていることを承知のうえで扱うこと。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PluginCapabilities {
    /// Surge XT: 1 以上、Dexed: 0。0 なら input buffer を 1 本も渡さない。
    pub audio_input_ports: u32,
    /// Surge XT: 3、Dexed: 1（実測）。
    pub audio_output_ports: u32,
    /// output port 0 のチャンネル数。ステレオでなければ受け付けない。
    pub main_output_channels: u32,
    /// output port 0 が `IS_MAIN` を立てているか。main が先頭にない topology は未対応。
    pub main_output_is_main: bool,
    pub input_note_ports: u32,
    /// Surge XT: CLAP|MIDI、Dexed: MIDI のみ。
    pub input_note_dialects: NoteDialects,
}

/// note event をどの方言で組むか。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NoteEventDialect {
    Clap,
    Midi,
}

/// factory の descriptor を 1 件に決める。
///
/// descriptor が 2 件以上ある CLAP は「どれを使うか」を決める材料が無いので受け付けない。
/// Step 2 で config の `plugin_id` が届いたら、期待 ID との突き合わせをここへ足す。
pub fn select_descriptor(entry: &PluginEntry) -> Result<SelectedDescriptor> {
    let plugin_factory = entry
        .get_plugin_factory()
        .ok_or_else(|| anyhow::anyhow!("PluginFactory が見つからない"))?;
    let descriptors = plugin_factory
        .plugin_descriptors()
        .map(|descriptor| SelectedDescriptor {
            id: cstr_to_string(descriptor.id()),
            name: cstr_to_string(descriptor.name()),
            vendor: cstr_to_string(descriptor.vendor()),
            version: cstr_to_string(descriptor.version()),
        })
        .collect::<Vec<_>>();
    choose_descriptor(descriptors)
}

fn cstr_to_string(value: Option<&std::ffi::CStr>) -> String {
    value
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn choose_descriptor(descriptors: Vec<SelectedDescriptor>) -> Result<SelectedDescriptor> {
    if descriptors.len() > 1 {
        let ids = descriptors
            .iter()
            .map(|descriptor| descriptor.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "プラグインディスクリプタが {} 件あり、どれを使うか決められない (descriptor ID: {})",
            descriptors.len(),
            ids
        );
    }
    let selected = descriptors
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("プラグインディスクリプタが見つからない"))?;
    if selected.id.is_empty() {
        anyhow::bail!("プラグインIDが見つからない");
    }
    Ok(selected)
}

/// extension を読んで能力を確定させ、レンダリングループが扱える構成かを検証する。
///
/// `activate()` の前に、main thread から呼ぶこと。
pub(super) fn probe_capabilities(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    descriptor: &SelectedDescriptor,
) -> Result<PluginCapabilities> {
    let capabilities = read_capabilities(plugin_instance);
    validate_capabilities(&capabilities, descriptor)?;
    Ok(capabilities)
}

fn read_capabilities(plugin_instance: &mut PluginInstance<MidiRenderHost>) -> PluginCapabilities {
    let handle = plugin_instance.plugin_handle();
    // extension が無いプラグインは、その種類の port を 1 つも持たない（CLAP の規約）。
    let (audio_input_ports, audio_output_ports, main_output_channels, main_output_is_main) =
        match handle.get_extension::<PluginAudioPorts>() {
            Some(audio_ports) => {
                let mut buffer = AudioPortInfoBuffer::new();
                let main_output = audio_ports.get(&handle, 0, false, &mut buffer);
                let channels = main_output.as_ref().map_or(0, |info| info.channel_count);
                let is_main = main_output
                    .as_ref()
                    .is_some_and(|info| info.flags.contains(AudioPortFlags::IS_MAIN));
                (
                    audio_ports.count(&handle, true),
                    audio_ports.count(&handle, false),
                    channels,
                    is_main,
                )
            }
            None => (0, 0, 0, false),
        };
    let (input_note_ports, input_note_dialects) = match handle.get_extension::<PluginNotePorts>() {
        Some(note_ports) => {
            let count = note_ports.count(&handle, true);
            let mut buffer = NotePortInfoBuffer::new();
            let dialects = note_ports
                .get(&handle, 0, true, &mut buffer)
                .map(|info| info.supported_dialects)
                .unwrap_or_else(NoteDialects::empty);
            (count, dialects)
        }
        None => (0, NoteDialects::empty()),
    };
    PluginCapabilities {
        audio_input_ports,
        audio_output_ports,
        main_output_channels,
        main_output_is_main,
        input_note_ports,
        input_note_dialects,
    }
}

fn validate_capabilities(
    capabilities: &PluginCapabilities,
    descriptor: &SelectedDescriptor,
) -> Result<()> {
    if capabilities.audio_output_ports == 0 {
        anyhow::bail!(
            "audio output port を持たないプラグインは未対応 (descriptor ID: {}, 必要: ステレオの main output を 1 本以上)",
            descriptor.id
        );
    }
    if !capabilities.main_output_is_main {
        anyhow::bail!(
            "先頭の audio output port が main ではないプラグインは未対応 \
             (descriptor ID: {}, output port 数: {})",
            descriptor.id,
            capabilities.audio_output_ports
        );
    }
    if capabilities.main_output_channels != STEREO_CHANNELS {
        anyhow::bail!(
            "main output port が {} ch のプラグインは未対応 (descriptor ID: {}, 必要: {} ch)",
            capabilities.main_output_channels,
            descriptor.id,
            STEREO_CHANNELS
        );
    }
    if capabilities.input_note_ports == 0 {
        anyhow::bail!(
            "note input port を持たないプラグインは未対応 (descriptor ID: {}, 必要: 1 本以上)",
            descriptor.id
        );
    }
    resolve_note_dialect(capabilities, descriptor).map(|_| ())
}

/// 広告された dialect から、送る event の種類を決める。
///
/// CLAP note が使えるなら note_id を載せられる（voicing probe が成立する）ので優先する。
pub(super) fn resolve_note_dialect(
    capabilities: &PluginCapabilities,
    descriptor: &SelectedDescriptor,
) -> Result<NoteEventDialect> {
    if capabilities
        .input_note_dialects
        .contains(NoteDialects::CLAP)
    {
        return Ok(NoteEventDialect::Clap);
    }
    if capabilities
        .input_note_dialects
        .contains(NoteDialects::MIDI)
    {
        return Ok(NoteEventDialect::Midi);
    }
    anyhow::bail!(
        "note input port の dialect が CLAP でも MIDI でもないプラグインは未対応 \
         (descriptor ID: {}, 広告された dialect: {:?})",
        descriptor.id,
        capabilities.input_note_dialects
    )
}

#[cfg(test)]
mod tests;
