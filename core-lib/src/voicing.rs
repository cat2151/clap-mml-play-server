use clack_extensions::{
    params::{ParamInfoBuffer, PluginParams},
    voice_info::{PluginVoiceInfo, VoiceInfoFlags},
};
use clack_host::prelude::PluginInstance;
use serde::{Deserialize, Serialize};

use crate::host::MidiRenderHost;

pub(crate) const SURGE_PLUGIN_ID: &str = "org.surge-synth-team.surge-xt";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PatchVoicing {
    Mono,
    Poly,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SurgeVoicing {
    Mono,
    Poly,
    Mixed,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProbeReport {
    pub result: PatchVoicing,
    pub ended_note_ids: Vec<u32>,
    pub blocks: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VoiceInfoReport {
    pub voice_count: u32,
    pub voice_capacity: u32,
    pub supports_overlapping_notes: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SurgeParamsReport {
    pub scene_mode: String,
    pub active_scene: String,
    pub scene_a_play_mode: String,
    pub scene_b_play_mode: String,
    pub result: SurgeVoicing,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VoicingReport {
    pub decision: PatchVoicing,
    pub probe: ProbeReport,
    pub voice_info: Option<VoiceInfoReport>,
    pub surge: Option<SurgeParamsReport>,
    pub disagreement: bool,
}

#[derive(Default)]
struct SurgeParamValues {
    scene_mode: Option<String>,
    active_scene: Option<String>,
    scene_a_play_mode: Option<String>,
    scene_b_play_mode: Option<String>,
}

pub(crate) fn build_voicing_report(
    probe: ProbeReport,
    voice_info: Option<VoiceInfoReport>,
    surge: Option<SurgeParamsReport>,
) -> VoicingReport {
    let disagreement = surge.as_ref().is_some_and(|surge| match surge.result {
        SurgeVoicing::Mono => probe.result == PatchVoicing::Poly,
        SurgeVoicing::Poly | SurgeVoicing::Mixed => probe.result == PatchVoicing::Mono,
        SurgeVoicing::Unknown => false,
    });
    let decision = match surge.as_ref().map(|surge| surge.result) {
        Some(SurgeVoicing::Poly | SurgeVoicing::Mixed) => PatchVoicing::Poly,
        _ => probe.result,
    };
    VoicingReport {
        decision,
        probe,
        voice_info,
        surge,
        disagreement,
    }
}

pub(crate) fn read_plugin_diagnostics(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    plugin_id: &str,
) -> (Option<VoiceInfoReport>, Option<SurgeParamsReport>) {
    let voice_info = read_voice_info(plugin_instance);
    let surge = (plugin_id == SURGE_PLUGIN_ID)
        .then(|| read_surge_params(plugin_instance))
        .flatten();
    (voice_info, surge)
}

fn read_voice_info(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
) -> Option<VoiceInfoReport> {
    let extension = plugin_instance
        .plugin_handle()
        .get_extension::<PluginVoiceInfo>()?;
    let info = extension.get(&mut plugin_instance.plugin_handle())?;
    Some(VoiceInfoReport {
        voice_count: info.voice_count,
        voice_capacity: info.voice_capacity,
        supports_overlapping_notes: info
            .flags
            .contains(VoiceInfoFlags::SUPPORTS_OVERLAPPING_NOTES),
    })
}

fn read_surge_params(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
) -> Option<SurgeParamsReport> {
    let extension = plugin_instance
        .plugin_handle()
        .get_extension::<PluginParams>()?;
    let mut values = SurgeParamValues::default();
    let count = extension.count(&mut plugin_instance.plugin_handle());
    for index in 0..count {
        let mut buffer = ParamInfoBuffer::new();
        let Some(info) =
            extension.get_info(&mut plugin_instance.plugin_handle(), index, &mut buffer)
        else {
            continue;
        };
        let id = info.id;
        let name = String::from_utf8_lossy(info.name).into_owned();
        let module = String::from_utf8_lossy(info.module).into_owned();
        let Some(value) = extension.get_value(&mut plugin_instance.plugin_handle(), id) else {
            continue;
        };
        let mut text_buffer = [0_u8; 128];
        let Ok(text) = extension.value_to_text(
            &mut plugin_instance.plugin_handle(),
            id,
            value,
            &mut text_buffer,
        ) else {
            continue;
        };
        let text = String::from_utf8_lossy(text).trim().to_string();
        record_surge_param(&mut values, &name, &module, text);
    }
    values.into_report()
}

fn record_surge_param(values: &mut SurgeParamValues, name: &str, module: &str, value: String) {
    let name = name.to_ascii_lowercase();
    let module = module.to_ascii_lowercase();
    match name.as_str() {
        "scene mode" => values.scene_mode = Some(value),
        "active scene" => values.active_scene = Some(value),
        "a play mode" => values.scene_a_play_mode = Some(value),
        "b play mode" => values.scene_b_play_mode = Some(value),
        "play mode" if module.contains("scene a") => values.scene_a_play_mode = Some(value),
        "play mode" if module.contains("scene b") => values.scene_b_play_mode = Some(value),
        _ => {}
    }
}

impl SurgeParamValues {
    fn into_report(self) -> Option<SurgeParamsReport> {
        let scene_mode = self.scene_mode?;
        let active_scene = self.active_scene?;
        let scene_a_play_mode = self.scene_a_play_mode?;
        let scene_b_play_mode = self.scene_b_play_mode?;
        let result = classify_surge(
            &scene_mode,
            &active_scene,
            &scene_a_play_mode,
            &scene_b_play_mode,
        );
        Some(SurgeParamsReport {
            scene_mode,
            active_scene,
            scene_a_play_mode,
            scene_b_play_mode,
            result,
        })
    }
}

fn classify_play_mode(value: &str) -> PatchVoicing {
    match value.trim().to_ascii_lowercase().as_str() {
        "poly" => PatchVoicing::Poly,
        "mono"
        | "mono (single trigger)"
        | "mono (fingered portamento)"
        | "mono (single trigger & fingered portamento)"
        | "latch (monophonic)" => PatchVoicing::Mono,
        _ => PatchVoicing::Unknown,
    }
}

fn classify_surge(scene_mode: &str, active_scene: &str, a: &str, b: &str) -> SurgeVoicing {
    let a = classify_play_mode(a);
    let b = classify_play_mode(b);
    let scene_mode = scene_mode.trim().to_ascii_lowercase();
    if scene_mode == "single" {
        return match active_scene.trim().to_ascii_lowercase().as_str() {
            "a" | "0" => surge_from_single(a),
            "b" | "1" => surge_from_single(b),
            _ => SurgeVoicing::Unknown,
        };
    }
    if !matches!(scene_mode.as_str(), "key split" | "dual" | "channel split") {
        return SurgeVoicing::Unknown;
    }
    match (a, b) {
        (PatchVoicing::Mono, PatchVoicing::Mono) => SurgeVoicing::Mono,
        (PatchVoicing::Poly, PatchVoicing::Poly) => SurgeVoicing::Poly,
        (PatchVoicing::Mono, PatchVoicing::Poly) | (PatchVoicing::Poly, PatchVoicing::Mono) => {
            SurgeVoicing::Mixed
        }
        _ => SurgeVoicing::Unknown,
    }
}

fn surge_from_single(value: PatchVoicing) -> SurgeVoicing {
    match value {
        PatchVoicing::Mono => SurgeVoicing::Mono,
        PatchVoicing::Poly => SurgeVoicing::Poly,
        PatchVoicing::Unknown => SurgeVoicing::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_documented_mono_labels_are_mono() {
        for label in [
            "Mono",
            "Mono (Single Trigger)",
            "Mono (Fingered Portamento)",
            "Mono (Single Trigger & Fingered Portamento)",
            "Latch (Monophonic)",
        ] {
            assert_eq!(classify_play_mode(label), PatchVoicing::Mono);
        }
    }

    #[test]
    fn single_uses_the_active_scene() {
        assert_eq!(
            classify_surge("Single", "B", "Mono", "Poly"),
            SurgeVoicing::Poly
        );
    }

    #[test]
    fn actual_surge_display_casing_and_numeric_scene_are_supported() {
        assert_eq!(
            classify_surge("Single", "0", "Mono (single trigger)", "Poly"),
            SurgeVoicing::Mono
        );
        assert_eq!(
            classify_surge(
                "Key split",
                "1",
                "Mono (single trigger)",
                "Mono (single trigger & fingered portamento)",
            ),
            SurgeVoicing::Mono
        );
    }

    #[test]
    fn multi_scene_reports_mixed() {
        assert_eq!(
            classify_surge("Dual", "A", "Mono", "Poly"),
            SurgeVoicing::Mixed
        );
    }

    #[test]
    fn surge_poly_or_mixed_overrides_a_mono_probe() {
        let report = build_voicing_report(
            ProbeReport {
                result: PatchVoicing::Mono,
                ended_note_ids: vec![2],
                blocks: 1,
            },
            None,
            Some(SurgeParamsReport {
                scene_mode: "Dual".into(),
                active_scene: "A".into(),
                scene_a_play_mode: "Mono".into(),
                scene_b_play_mode: "Poly".into(),
                result: SurgeVoicing::Mixed,
            }),
        );
        assert_eq!(report.decision, PatchVoicing::Poly);
        assert!(report.disagreement);
    }
}
