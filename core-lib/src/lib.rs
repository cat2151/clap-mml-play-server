pub mod host;
pub mod midi;
pub mod patch_list;
pub mod pipeline;
pub mod render;
mod workspace_update;

#[derive(Debug, Clone)]
pub struct CoreConfig {
    pub output_midi: String,
    pub output_wav: String,
    pub sample_rate: f64,
    pub buffer_size: usize,
    pub patch_path: Option<String>,
    pub patches_dir: Option<String>,
    pub random_patch: bool,
}

pub use host::load_entry;
pub use patch_list::{collect_patches, to_relative};
pub use pipeline::{
    encode_wav_i16, ensure_cmrt_dir, ensure_daw_dir, ensure_phrase_dir, mml_render,
    mml_render_for_cache, mml_render_for_cache_with_options, mml_render_stateless,
    mml_render_stateless_with_options, mml_render_with_options, mml_str_to_smf_bytes, mml_to_play,
    mml_to_play_with_options, mml_to_smf_bytes, play_samples, prepare_realtime_play,
    smf_playback_schedule_with_options, smf_render_stateless_with_options, write_wav,
    PreparedRealtimePlay, RenderOptions, RenderPreroll,
};
pub use render::{create_plugin_instance, RealtimePlaybackSchedule, RealtimeRenderer};
pub use workspace_update::{check_workspace_update, run_workspace_update};
