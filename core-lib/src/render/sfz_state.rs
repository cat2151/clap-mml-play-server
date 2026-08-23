//! Sforzando arbitrary-SFZ loader based on the plugin's vendor state container.

use std::path::Path;

use anyhow::{Context, Result};
use clack_host::prelude::PluginInstance;

use super::patch_state::load_plugin_state;
use super::RealtimeRenderer;
use crate::host::MidiRenderHost;
use crate::sforzando::{
    resolve_sforzando_program, sforzando_state_blob, SforzandoProgramRef, SFORZANDO_PLUGIN_ID,
};

impl RealtimeRenderer {
    pub(super) fn load_sfz_state(&mut self, patch_path: &str) -> Result<()> {
        ensure_sforzando_capable(&self.plugin_id, patch_path)?;
        // Resolve and build before stopping audio. A missing mapping must leave the currently
        // running processor and patch untouched.
        let program = resolve_sforzando_program(Path::new(patch_path)).with_context(|| {
            format!(
                "SFZ program resolution に失敗 requested='{patch_path}' plugin_id='{}'",
                self.plugin_id
            )
        })?;
        let init_state = self.init_state.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "Sforzando init state がない requested='{patch_path}' canonical='{}' plugin_id='{}' source='{}'",
                program.sfz_path.display(), self.plugin_id, program.source
            )
        })?;
        let state = state_for_program(init_state, patch_path, &self.plugin_id, &program)?;

        let processor = self
            .processor
            .take()
            .ok_or_else(|| anyhow::anyhow!("SFZ 切替時に audio processor がない"))?;
        let stopped = processor.stop_processing();
        let load_result =
            load_plugin_state(self.plugin_instance_mut(), &state).with_context(|| {
                program_context(
                    "SFZ state.load に失敗",
                    patch_path,
                    &self.plugin_id,
                    &program,
                )
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
            (Ok(()), Err(restart_error)) => Err(restart_error.context(program_context(
                "SFZ state.load 後に processor を復旧できない",
                patch_path,
                &self.plugin_id,
                &program,
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
) -> Result<()> {
    ensure_sforzando_capable(plugin_id, patch_path)?;
    let program = resolve_sforzando_program(Path::new(patch_path)).with_context(|| {
        format!("SFZ program resolution に失敗 requested='{patch_path}' plugin_id='{plugin_id}'")
    })?;
    let state = state_for_program(init_state, patch_path, plugin_id, &program)?;
    load_plugin_state(plugin_instance, &state).with_context(|| {
        program_context(
            "SFZ initial state.load に失敗",
            patch_path,
            plugin_id,
            &program,
        )
    })
}

fn state_for_program(
    init_state: &[u8],
    requested: &str,
    plugin_id: &str,
    program: &SforzandoProgramRef,
) -> Result<Vec<u8>> {
    sforzando_state_blob(init_state, program).with_context(|| {
        program_context(
            "Sforzando init state template から state を構築できない",
            requested,
            plugin_id,
            program,
        )
    })
}

fn program_context(
    action: &str,
    requested: &str,
    plugin_id: &str,
    program: &SforzandoProgramRef,
) -> String {
    format!(
        "{action} requested='{requested}' canonical='{}' plugin_id='{plugin_id}' source='{}' bank_id='{}' bank_version='{}' program='{}'",
        program.sfz_path.display(),
        program.source,
        program.bank_id,
        program.bank_version,
        program.program_name
    )
}

pub(super) fn ensure_sforzando_capable(plugin_id: &str, patch_path: &str) -> Result<()> {
    if plugin_id == SFORZANDO_PLUGIN_ID {
        return Ok(());
    }
    anyhow::bail!(
        "SFZ '{patch_path}' は plugin_id = '{SFORZANDO_PLUGIN_ID}' でしか読めない（いま載っているのは '{plugin_id}'）"
    )
}

#[cfg(test)]
mod tests;
