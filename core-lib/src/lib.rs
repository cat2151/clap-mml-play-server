pub mod host;
pub mod midi;
pub mod patch_list;
pub mod pipeline;
pub mod render;

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
    ensure_cmrt_dir, ensure_daw_dir, ensure_phrase_dir, mml_render, mml_render_for_cache,
    mml_str_to_smf_bytes, mml_to_play, mml_to_smf_bytes, play_samples, write_wav,
};
