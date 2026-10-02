//! 音色の一覧（[`plugin_presets::patch_list`]）を、診断を host process の sink へ流す形で公開する。

use anyhow::Result;
use std::path::PathBuf;

pub use plugin_presets::patch_list::{to_relative, CollectedPatch, MergedPatches};

use crate::logging::emit_diagnostic;

/// [`plugin_presets::patch_list::collect_patches`] と同じ。
pub fn collect_patches(patches_dir: &str) -> Result<Vec<PathBuf>> {
    plugin_presets::patch_list::collect_patches(patches_dir, emit_diagnostic)
}

/// [`plugin_presets::patch_list::collect_patch_listing`] と同じ。
pub fn collect_patch_listing(patches_dir: &str) -> Result<Vec<CollectedPatch>> {
    plugin_presets::patch_list::collect_patch_listing(patches_dir, emit_diagnostic)
}

#[cfg(test)]
mod tests;
