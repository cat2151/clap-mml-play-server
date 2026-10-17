#[cfg(feature = "host")]
pub mod audio_effect;
#[cfg(feature = "host")]
mod audio_plugin;
#[cfg(feature = "host")]
pub mod boot_log;
#[cfg(feature = "host")]
pub mod cache_wav;
#[cfg(feature = "host")]
pub mod downloaded_patches;
#[cfg(feature = "host")]
pub mod effect;
#[cfg(feature = "host")]
pub mod effect_plugins;
#[cfg(feature = "host")]
pub mod floe;
#[cfg(feature = "host")]
pub mod host;
#[cfg(feature = "host")]
mod logging;
#[cfg(feature = "host")]
pub mod midi;
#[cfg(feature = "host")]
pub mod patch_list;
#[cfg(feature = "host")]
pub mod patch_state_prepare;
pub mod path_identity;
#[cfg(feature = "host")]
pub mod pipeline;
#[cfg(feature = "host")]
pub mod plugin_catalog;
#[cfg(feature = "host")]
pub mod render;
#[cfg(feature = "host")]
pub mod sforzando;
#[cfg(feature = "host")]
pub mod six_sines;
#[cfg(feature = "host")]
pub mod six_sines_factory;
#[cfg(feature = "host")]
pub mod surge_data;
#[cfg(feature = "host")]
pub mod tyrelln6;
#[cfg(feature = "host")]
pub mod voicing;
#[cfg(feature = "host")]
mod workspace_update;

#[cfg(feature = "host")]
pub use plugin_presets::dexed as dx7;
#[cfg(feature = "host")]
pub use plugin_presets::dragonfly as dragonfly_preset;
#[cfg(feature = "host")]
pub use plugin_presets::juce_value_tree;
#[cfg(feature = "host")]
pub use plugin_presets::shu as shu_preset;
#[cfg(feature = "host")]
pub use plugin_presets::surge_fx as surge_fx_preset;
#[cfg(feature = "host")]
pub use plugin_presets::tone3000 as tone3000_preset;
#[cfg(feature = "host")]
pub use plugin_presets::vaporizer2 as vvp;
#[cfg(feature = "host")]
pub use plugin_presets::voyage_voyage as voyage_voyage_preset;

#[cfg(feature = "host")]
/// レンダリング 1 回ぶんの設定。
///
/// `Default` を derive してあるのは、テストの構造体リテラルが `..Default::default()` で
/// 済むようにするため。フィールドを足しても別 repo（clap-mml-render-tui）のテストが
/// 壊れなくなる。**本番のリテラル（各サーバーの `core_config_from_runtime` と
/// TUI の `core_config_from_config`）では省略せず全フィールドを書くこと**。
/// 省略すると、新しいフィールドの配線漏れがコンパイルエラーにならない。
#[derive(Debug, Clone, Default)]
pub struct CoreConfig {
    /// config の `plugin_id`。descriptor を複数持つ CLAP で 1 件を名指しするために使う。
    /// `None` なら descriptor が 1 件のときだけ受け付ける（[`render::select_descriptor`]）。
    pub plugin_id: Option<String>,
    pub output_midi: String,
    pub output_wav: String,
    pub sample_rate: f64,
    pub buffer_size: usize,
    pub patch_path: Option<String>,
    /// display（相対の patch 文字列）と絶対パスを行き来する基点。
    pub patch_base: PatchBase,
    pub random_patch: bool,
}

#[cfg(feature = "host")]
pub use audio_effect::{
    builtin_effect_plugins, effect_chain_spec_from_embedded_json, embedded_json_has_effect_chain,
    AudioEffectCatalog, AudioEffectPluginInfo, AudioEffectPreset, EffectChainSpec, EffectStageSpec,
    PresetLocation, EFFECT_CHAIN_JSON_KEY, EFFECT_STAGE_BYPASS_JSON_KEY,
};
#[cfg(feature = "host")]
pub use audio_plugin::{
    patch_lookup_candidates, patch_sort_metadata, plugin_voicing_source, AudioPatch,
    AudioPluginCatalog, AudioPluginInfo, PatchRef, PatchSortMetadata, PatchVoicingHint, PluginKey,
    PluginVoicingSource, RouteError,
};
#[cfg(feature = "host")]
pub use boot_log::{log_boot, log_boot_fatal};
#[cfg(feature = "host")]
pub use cmrt_server_config::DrumKitNote;
#[cfg(feature = "host")]
pub use cmrt_server_config::{lexical_absolute, PatchBase};
#[cfg(feature = "host")]
pub use downloaded_patches::{prepare_downloaded_patches, PatchDownload};
#[cfg(feature = "host")]
pub use dx7::{
    cartridge_program_component, is_cartridge_patch_path, parse_cartridge_patch_path,
    parse_dx7_cartridge, CartridgePatchPath, Dx7Cartridge, DEXED_PLUGIN_ID, DX7_BULK_DUMP_LEN,
    DX7_PROGRAMS_PER_CARTRIDGE,
};
#[cfg(feature = "host")]
pub use effect_plugins::EffectPlugins;
#[cfg(feature = "host")]
pub use floe::{
    floe_note_assignments, floe_preset_is_percussion, is_floe_preset_path, FLOE_PLUGIN_ID,
};
#[cfg(feature = "host")]
pub use host::{
    builtin_plugin_path, load_builtin_entry, load_entry, PluginEntry, BUILTIN_PLUGIN_PATH_PREFIX,
};
#[cfg(feature = "host")]
pub use logging::{set_log_sink, LogSink};
#[cfg(feature = "host")]
pub use patch_list::{
    collect_patch_listing, collect_patches, to_relative, CollectedPatch, MergedPatches,
};
#[cfg(feature = "host")]
pub use patch_state_prepare::{prepare_clap_patch_state, PatchStateError};
#[cfg(feature = "host")]
pub use pipeline::{
    embedded_patch_ref, encode_wav_i16, ensure_cmrt_dir, ensure_daw_dir, ensure_phrase_dir,
    mml_render, mml_render_for_cache, mml_render_for_cache_with_effects,
    mml_render_for_cache_with_options, mml_render_stateless, mml_render_stateless_with_effects,
    mml_render_stateless_with_options, mml_render_with_effects, mml_render_with_options,
    mml_str_to_smf_bytes, mml_to_play, mml_to_play_with_options, mml_to_smf_bytes, play_samples,
    prepare_realtime_play, smf_playback_schedule_with_options, smf_render_stateless_with_options,
    write_wav, EffectEntryLoader, PreparedRealtimePlay, RenderEffects, RenderOptions,
    RenderPreroll,
};
#[cfg(feature = "host")]
pub use plugin_catalog::{kind_for_patch, plugin_kinds, PatchBases, PluginKind};
#[cfg(feature = "host")]
pub use render::{
    create_renderers_parallel, plugin_requires_serial_instantiation, probe_plugin_capabilities,
    select_descriptor, LiveMidiEvent, RealtimePlaybackSchedule, RealtimeRenderer, RendererCreated,
    RendererHandoff, RendererInitTiming, RendererSpec, SelectedDescriptor,
};
#[cfg(feature = "host")]
pub use render::{PluginProbeReport, ProbedDescriptor};
#[cfg(feature = "host")]
pub use sforzando::{
    is_ariax_patch_path, is_sforzando_patch_path, is_sfz_patch_path, sfz_is_drum_kit,
    sfz_note_assignments, sfz_one_shot_notes, sfz_sample_weight, SfzSampleWeight,
    SFORZANDO_PLUGIN_ID,
};
#[cfg(feature = "host")]
pub use six_sines::{is_six_sines_patch_path, SIX_SINES_PLUGIN_ID};
#[cfg(feature = "host")]
pub use surge_data::{
    apply_minimal_surge_data_home, plugin_is_surge, MinimalSurgeDataHome, SURGE_XT_PLUGIN_ID,
};
#[cfg(feature = "host")]
pub use tyrelln6::{is_tyrelln6_patch_path, TYRELLN6_PLUGIN_ID};
#[cfg(feature = "host")]
pub use voicing::{PatchVoicing, VoicingReport};
#[cfg(feature = "host")]
pub use vvp::{
    is_vvp_patch_path, parse_vvp_header, read_vvp_header, vvp_state_blob, VvpHeader,
    VAPORIZER2_PLUGIN_ID,
};
#[cfg(feature = "host")]
pub use workspace_update::{check_workspace_update, run_workspace_update};
