pub mod dx7;
pub mod host;
pub mod midi;
pub mod patch_list;
pub mod pipeline;
pub mod plugin_catalog;
pub mod render;
pub mod surge_data;
pub mod voicing;
mod workspace_update;

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
    pub patches_dir: Option<String>,
    pub random_patch: bool,
}

pub use dx7::{
    cartridge_program_component, is_cartridge_patch_path, parse_cartridge_patch_path,
    parse_dx7_cartridge, CartridgePatchPath, Dx7Cartridge, DEXED_PLUGIN_ID, DX7_BULK_DUMP_LEN,
    DX7_PROGRAMS_PER_CARTRIDGE,
};
pub use host::{load_entry, PluginEntry};
pub use patch_list::{collect_patches, to_relative};
pub use pipeline::{
    embedded_patch_ref, encode_wav_i16, ensure_cmrt_dir, ensure_daw_dir, ensure_phrase_dir,
    mml_render, mml_render_for_cache, mml_render_for_cache_with_options, mml_render_stateless,
    mml_render_stateless_with_options, mml_render_with_options, mml_str_to_smf_bytes, mml_to_play,
    mml_to_play_with_options, mml_to_smf_bytes, play_samples, prepare_realtime_play,
    smf_playback_schedule_with_options, smf_render_stateless_with_options, write_wav,
    PreparedRealtimePlay, RenderOptions, RenderPreroll,
};
pub use plugin_catalog::{kind_for_patch, plugin_kinds, PatchBases, PluginKind};
pub use render::{
    create_renderers_parallel, select_descriptor, LiveMidiEvent, RealtimePlaybackSchedule,
    RealtimeRenderer, RendererCreated, RendererHandoff, RendererInitTiming, RendererSpec,
    SelectedDescriptor,
};
pub use surge_data::{
    apply_minimal_surge_data_home, plugin_is_surge, MinimalSurgeDataHome, SURGE_XT_PLUGIN_ID,
};
pub use voicing::{PatchVoicing, VoicingReport};
pub use workspace_update::{check_workspace_update, run_workspace_update};
