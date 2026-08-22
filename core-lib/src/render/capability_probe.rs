//! プラグイン 1 本ぶんの能力を、実際に load して測る。
//!
//! ADR 0001 の実測表と ADR 0006 の判断（CLAP の generic な preset API には乗れない）は、
//! どちらも「本当に `get_extension` / `get_factory` を呼んだ結果」に基づいている。
//! **バイナリ内の文字列検索で能力を判定してはならない**（ADR 0006 の罠）ので、
//! 対応プラグインを増やすたびにここを通して測り直すこと。
//!
//! レンダリングループが使う [`super::descriptor::PluginCapabilities`] は
//! 「port と event を組み立てるのに要る最小限」だけを持つ。ここはそれに加えて
//! 表へ載せたいもの（descriptor 一覧・features・factory と extension の有無・
//! params 数・voice-info）まで読むので、型を分けてある。

use std::ffi::{c_void, CStr};

use anyhow::{Context as _, Result};
use clack_extensions::note_ports::{NoteDialects, PluginNotePorts};
use clack_extensions::params::PluginParams;
use clack_host::prelude::*;
use serde::Serialize;

use super::descriptor::{
    read_capabilities, select_descriptor, validate_capabilities, SelectedDescriptor,
};
use super::instance::create_plugin_instance_without_patch;
use crate::host::{load_entry, MidiRenderHost};

/// entry の `get_factory` へ問い合わせる factory ID。
///
/// preset-discovery は版が何度も変わっているので、**確定版と draft を両方**当てる。
/// 片方だけ見て「無い」と書くと、draft で出しているプラグインを取り逃す。
const PROBED_FACTORY_IDS: &[&CStr] = &[
    c"clap.plugin-factory",
    c"clap.plugin-invalidation-factory",
    c"clap.preset-discovery-factory/2",
    c"clap.preset-discovery-factory/draft/2",
    c"clap.preset-discovery-factory/draft/1",
    c"clap.preset-discovery-factory",
];

/// instance の `get_extension` へ問い合わせる extension ID。
///
/// preset-load も同じ理由で確定版と draft を並べる。clap-juce-extensions を使う
/// プラグイン（Surge XT / Vaporizer2）は、opt-in していなくてもバイナリ内に
/// これらの文字列を持っている。**NULL が返るかどうかだけが答え。**
const PROBED_EXTENSION_IDS: &[&CStr] = &[
    c"clap.audio-ports",
    c"clap.audio-ports-config",
    c"clap.gui",
    c"clap.latency",
    c"clap.note-name",
    c"clap.note-ports",
    c"clap.params",
    c"clap.preset-load/2",
    c"clap.preset-load.draft/2",
    c"clap.preset-load.draft/0",
    c"clap.preset-load",
    c"clap.remote-controls/2",
    c"clap.render",
    c"clap.state",
    c"clap.state-context/2",
    c"clap.tail",
    c"clap.thread-pool",
    c"clap.timer-support",
    c"clap.voice-info",
];

/// factory が広告した descriptor 1 件。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProbedDescriptor {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub features: Vec<String>,
}

/// CLAP 1 本ぶんの実測結果。
#[derive(Clone, Debug, Serialize)]
pub struct PluginProbeReport {
    pub plugin_path: String,
    /// factory が広告した descriptor の全件。2 件以上なら config の `plugin_id` が必須。
    pub descriptors: Vec<ProbedDescriptor>,
    /// 実際に instance を作った descriptor。
    pub selected: SelectedDescriptor,
    /// 非 NULL が返った factory ID。
    pub factories: Vec<String>,
    /// 非 NULL が返った extension ID。
    pub extensions: Vec<String>,
    pub audio_input_ports: u32,
    pub audio_output_ports: u32,
    pub main_output_channels: u32,
    pub main_output_is_main: bool,
    pub input_note_ports: u32,
    pub output_note_ports: u32,
    /// note input port 0 が広告した dialect。
    pub input_note_dialects: Vec<String>,
    pub param_count: u32,
    /// 受け入れ条件（`validate_capabilities`）に落ちた理由。`None` なら通った。
    pub rejected: Option<String>,
}

impl PluginProbeReport {
    /// ADR へ貼れる 1 本ぶんの表。
    pub fn to_text(&self) -> String {
        let mut lines = vec![
            format!("plugin_path        : {}", self.plugin_path),
            format!("descriptor 数      : {}", self.descriptors.len()),
        ];
        for descriptor in &self.descriptors {
            lines.push(format!(
                "  id={} name={} vendor={} version={} features=[{}]",
                descriptor.id,
                descriptor.name,
                descriptor.vendor,
                descriptor.version,
                descriptor.features.join(", ")
            ));
        }
        lines.extend([
            format!("selected           : {}", self.selected.log_fields()),
            format!("audio input port   : {}", self.audio_input_ports),
            format!("audio output port  : {}", self.audio_output_ports),
            format!(
                "main (port 0)      : {} ch, {}",
                self.main_output_channels,
                if self.main_output_is_main {
                    "IS_MAIN"
                } else {
                    "IS_MAIN でない"
                }
            ),
            format!(
                "note port          : in {} / out {}",
                self.input_note_ports, self.output_note_ports
            ),
            format!(
                "note dialect       : {}",
                join_or_none(&self.input_note_dialects, " | ")
            ),
            format!("params             : {}", self.param_count),
            format!("voice-info         : {}", self.voice_info_row()),
            format!(
                "factory (非 NULL)  : {}",
                join_or_none(&self.factories, ", ")
            ),
            format!(
                "extension (非 NULL): {}",
                join_or_none(&self.extensions, ", ")
            ),
            format!(
                "受け入れ条件       : {}",
                match &self.rejected {
                    Some(reason) => format!("NG — {reason}"),
                    None => "OK".to_string(),
                }
            ),
        ]);
        lines.join("\n")
    }
}

impl PluginProbeReport {
    /// voice-info は「広告しているか」だけを表に載せる。
    ///
    /// `clap_plugin_voice_info.get()` は activate 後でないと値を返さない（Surge XT は
    /// 実際に警告を出す）。ここは activate 前の probe なので、**値を読もうとすると
    /// 広告しているプラグインまで「なし」に見える**。広告の有無なら extension の
    /// 非 NULL でそのまま分かる。
    fn voice_info_row(&self) -> &'static str {
        if self
            .extensions
            .iter()
            .any(|extension| extension == "clap.voice-info")
        {
            return "広告あり（値は activate 後でないと読めない）";
        }
        "なし"
    }
}

fn join_or_none(values: &[String], separator: &str) -> String {
    if values.is_empty() {
        return "(無し)".to_string();
    }
    values.join(separator)
}

/// CLAP を 1 本 load し、instance を 1 つ作って能力を測る。
///
/// `plugin_id` は config の `plugin_id` と同じ意味。descriptor が 2 件以上ある CLAP では
/// これが無いと 1 件に決められない（[`select_descriptor`]）。
///
/// `activate()` の前に main thread から呼ぶこと。instance は測り終えたら捨てる。
pub fn probe_plugin_capabilities(
    plugin_path: &str,
    plugin_id: Option<&str>,
) -> Result<PluginProbeReport> {
    let entry = load_entry(plugin_path)?;
    let descriptors = read_descriptors(&entry);
    let selected = select_descriptor(&entry, plugin_id)
        .with_context(|| format!("plugin_path={plugin_path}"))?;
    let factories = probe_factories(&entry);
    let mut plugin_instance = create_plugin_instance_without_patch(&entry, &selected)
        .with_context(|| format!("plugin_path={plugin_path}"))?;

    let capabilities = read_capabilities(&mut plugin_instance);
    // 受け入れ条件は「落ちたら測定終了」ではなく測定結果の 1 項目として持つ。
    // 落ちた理由まで表に残らないと、対応可否の判断がここで止まってしまう。
    let rejected = validate_capabilities(&capabilities, &selected)
        .err()
        .map(|error| format!("{error:#}"));
    let extensions = probe_extensions(&mut plugin_instance);
    let output_note_ports = read_output_note_ports(&mut plugin_instance);
    let param_count = read_param_count(&mut plugin_instance);

    Ok(PluginProbeReport {
        plugin_path: plugin_path.to_string(),
        descriptors,
        selected,
        factories,
        extensions,
        audio_input_ports: capabilities.audio_input_ports,
        audio_output_ports: capabilities.audio_output_ports,
        main_output_channels: capabilities.main_output_channels,
        main_output_is_main: capabilities.main_output_is_main,
        input_note_ports: capabilities.input_note_ports,
        output_note_ports,
        input_note_dialects: dialect_names(capabilities.input_note_dialects),
        param_count,
        rejected,
    })
}

fn read_descriptors(entry: &PluginEntry) -> Vec<ProbedDescriptor> {
    let Some(plugin_factory) = entry.get_plugin_factory() else {
        return Vec::new();
    };
    plugin_factory
        .plugin_descriptors()
        .map(|descriptor| ProbedDescriptor {
            id: cstr_to_string(descriptor.id()),
            name: cstr_to_string(descriptor.name()),
            vendor: cstr_to_string(descriptor.vendor()),
            version: cstr_to_string(descriptor.version()),
            features: descriptor
                .features()
                .map(|feature| feature.to_string_lossy().into_owned())
                .collect(),
        })
        .collect()
}

fn cstr_to_string(value: Option<&CStr>) -> String {
    value
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// entry の `get_factory` を ID ごとに呼び、非 NULL のものだけ残す。
///
/// clack が wrapper を持たない factory（preset-discovery）も測りたいので、
/// 型付き API ではなく raw の関数ポインタを直に呼ぶ。
fn probe_factories(entry: &PluginEntry) -> Vec<String> {
    let Some(get_factory) = entry.raw_entry().get_factory else {
        return Vec::new();
    };
    PROBED_FACTORY_IDS
        .iter()
        .filter(|id| {
            // SAFETY: entry は load 済みで生きている。CLAP の契約により、知らない
            // factory ID には NULL を返すだけなので、任意の ID を渡してよい。
            let pointer: *const c_void = unsafe { get_factory(id.as_ptr()) };
            !pointer.is_null()
        })
        .map(|id| id.to_string_lossy().into_owned())
        .collect()
}

/// instance の `get_extension` を ID ごとに呼び、非 NULL のものだけ残す。
fn probe_extensions(plugin_instance: &mut PluginInstance<MidiRenderHost>) -> Vec<String> {
    let handle = plugin_instance.plugin_handle();
    let Some(get_extension) = handle.as_raw().get_extension else {
        return Vec::new();
    };
    PROBED_EXTENSION_IDS
        .iter()
        .filter(|id| {
            // SAFETY: instance は生きていて、ここは main thread。CLAP の契約により、
            // 知らない extension ID には NULL を返すだけ。
            let pointer: *const c_void = unsafe { get_extension(handle.as_raw_ptr(), id.as_ptr()) };
            !pointer.is_null()
        })
        .map(|id| id.to_string_lossy().into_owned())
        .collect()
}

fn read_output_note_ports(plugin_instance: &mut PluginInstance<MidiRenderHost>) -> u32 {
    let handle = plugin_instance.plugin_handle();
    handle
        .get_extension::<PluginNotePorts>()
        .map_or(0, |note_ports| note_ports.count(&handle, false))
}

fn read_param_count(plugin_instance: &mut PluginInstance<MidiRenderHost>) -> u32 {
    let handle = plugin_instance.plugin_handle();
    handle
        .get_extension::<PluginParams>()
        .map_or(0, |params| params.count(&handle))
}

fn dialect_names(dialects: NoteDialects) -> Vec<String> {
    [
        (NoteDialects::CLAP, "CLAP"),
        (NoteDialects::MIDI, "MIDI"),
        (NoteDialects::MIDI_MPE, "MIDI_MPE"),
        (NoteDialects::MIDI2, "MIDI2"),
    ]
    .into_iter()
    .filter(|(flag, _)| dialects.contains(*flag))
    .map(|(_, name)| name.to_string())
    .collect()
}

#[cfg(test)]
mod tests;
