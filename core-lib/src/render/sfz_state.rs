//! Sforzando loader for arbitrary `.sfz` and ARIA `.ariax` presets, based on the plugin's
//! vendor state container.

use std::path::Path;

use anyhow::{Context, Result};
use clack_host::prelude::PluginInstance;

use super::patch_state::load_plugin_state;
use super::RealtimeRenderer;
use crate::host::MidiRenderHost;
use crate::sforzando::{
    is_ariax_patch_path, resolve_sforzando_preset, resolve_sforzando_program,
    sforzando_ariax_state_blob, sforzando_state_blob, SforzandoPresetRef, SforzandoProgramRef,
    SfzStreaming, SFORZANDO_PLUGIN_ID,
};

/// A requested Sforzando patch after its program coordinate has been verified.
enum SforzandoPatch {
    Program(SforzandoProgramRef),
    Preset(SforzandoPresetRef),
}

impl SforzandoPatch {
    fn resolve(patch_path: &str, plugin_id: &str) -> Result<Self> {
        let path = Path::new(patch_path);
        let resolved = if is_ariax_patch_path(patch_path) {
            resolve_sforzando_preset(path).map(Self::Preset)
        } else {
            resolve_sforzando_program(path).map(Self::Program)
        };
        resolved.with_context(|| {
            format!(
                "Sforzando program resolution に失敗 requested='{patch_path}' plugin_id='{plugin_id}'"
            )
        })
    }

    fn program(&self) -> &SforzandoProgramRef {
        match self {
            Self::Program(program) => program,
            Self::Preset(preset) => &preset.program,
        }
    }

    fn state(
        &self,
        init_state: &[u8],
        requested: &str,
        plugin_id: &str,
        streaming: SfzStreaming,
    ) -> Result<Vec<u8>> {
        match self {
            Self::Program(program) => sforzando_state_blob(init_state, program, streaming),
            Self::Preset(preset) => sforzando_ariax_state_blob(init_state, &preset.xml, streaming),
        }
        .with_context(|| {
            self.context(
                "Sforzando init state template から state を構築できない",
                requested,
                plugin_id,
            )
        })
    }

    fn context(&self, action: &str, requested: &str, plugin_id: &str) -> String {
        let program = self.program();
        let preset = match self {
            Self::Program(_) => String::new(),
            Self::Preset(preset) => format!(" preset='{}'", preset.ariax_path.display()),
        };
        format!(
            "{action} requested='{requested}'{preset} canonical='{}' plugin_id='{plugin_id}' source='{}' bank_id='{}' bank_version='{}' program='{}'",
            program.sfz_path.display(),
            program.source,
            program.bank_id,
            program.bank_version,
            program.program_name
        )
    }
}

impl RealtimeRenderer {
    pub(super) fn load_sfz_state(&mut self, patch_path: &str) -> Result<()> {
        ensure_sforzando_capable(&self.plugin_id, patch_path)?;
        // Resolve and build before stopping audio. A missing mapping must leave the currently
        // running processor and patch untouched.
        let patch = SforzandoPatch::resolve(patch_path, &self.plugin_id)?;
        let init_state = self.init_state.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "{}",
                patch.context("Sforzando init state がない", patch_path, &self.plugin_id)
            )
        })?;
        let state = patch.state(
            init_state,
            patch_path,
            &self.plugin_id,
            SfzStreaming::PluginDefault,
        )?;
        // ARIA keeps the Params of a program that stays loaded, and a `.sfz` state carries no
        // Params. Loading the init state first makes every switch start from the program's
        // defaults, so `.ariax` → `.sfz` of the same program does not keep the preset's sound.
        let init_state = init_state.to_vec();

        let processor = self
            .processor
            .take()
            .ok_or_else(|| anyhow::anyhow!("SFZ 切替時に audio processor がない"))?;
        let stopped = processor.stop_processing();
        let load_result = load_plugin_state(self.plugin_instance_mut(), &init_state)
            .with_context(|| {
                patch.context(
                    "SFZ 切替前の init state.load に失敗",
                    patch_path,
                    &self.plugin_id,
                )
            })
            .and_then(|()| {
                load_plugin_state(self.plugin_instance_mut(), &state).with_context(|| {
                    patch.context("SFZ state.load に失敗", patch_path, &self.plugin_id)
                })
            });
        let restart_result = stopped
            .start_processing()
            .map_err(|error| anyhow::anyhow!("SFZ 切替後の start_processing に失敗: {error:?}"));

        match (load_result, restart_result) {
            (Ok(()), Ok(processor)) => {
                self.processor = Some(processor);
                Ok(())
            }
            (Err(load_error), Ok(processor)) => {
                self.processor = Some(processor);
                Err(load_error)
            }
            (Ok(()), Err(restart_error)) => Err(restart_error.context(patch.context(
                "SFZ state.load 後に processor を復旧できない",
                patch_path,
                &self.plugin_id,
            ))),
            (Err(load_error), Err(restart_error)) => Err(anyhow::anyhow!(
                "{load_error:#}; 加えて processor restart に失敗: {restart_error:#}"
            )),
        }
    }
}

pub(super) fn load_initial_sfz_state(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    init_state: &[u8],
    patch_path: &str,
    plugin_id: &str,
    streaming: SfzStreaming,
) -> Result<()> {
    ensure_sforzando_capable(plugin_id, patch_path)?;
    let patch = SforzandoPatch::resolve(patch_path, plugin_id)?;
    let state = patch.state(init_state, patch_path, plugin_id, streaming)?;
    load_plugin_state(plugin_instance, &state)
        .with_context(|| patch.context("SFZ initial state.load に失敗", patch_path, plugin_id))
}

pub(super) fn ensure_sforzando_capable(plugin_id: &str, patch_path: &str) -> Result<()> {
    if plugin_id == SFORZANDO_PLUGIN_ID {
        return Ok(());
    }
    anyhow::bail!(
        "Sforzando patch '{patch_path}' は plugin_id = '{SFORZANDO_PLUGIN_ID}' でしか読めない（いま載っているのは '{plugin_id}'）"
    )
}

#[cfg(test)]
mod tests;
