//! OB-Xf の FXP program を CLAP state に包む。Surge の sub3 state とは別形式。

use anyhow::{ensure, Context, Result};
use clack_host::prelude::*;
use cmrt_server_config::{OBXF_PATCH_TAG, OBXF_PLUGIN_ID, SURGE_XT_PLUGIN_ID};

use crate::host::MidiRenderHost;

pub(super) fn ensure_state_file_owner(plugin_id: &str, tag: &str) -> Result<()> {
    let expected = match plugin_id {
        OBXF_PLUGIN_ID => OBXF_PATCH_TAG,
        SURGE_XT_PLUGIN_ID => "",
        _ => return Ok(()),
    };
    ensure!(
        tag == expected,
        "FXP のタグ ':{tag}' は plugin '{plugin_id}' 用ではない（期待値 ':{expected}'）"
    );
    Ok(())
}

pub(super) fn load_obxf_preset(
    instance: &mut PluginInstance<MidiRenderHost>,
    patch_path: &str,
) -> Result<()> {
    let path = crate::lexical_absolute(std::path::Path::new(patch_path))?;
    ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("fxp")),
        "OB-Xf は .fxp の音色を要求する"
    );
    ensure!(
        path.is_file(),
        "OB-Xf の音色ファイルが無い: {}",
        path.display()
    );
    let bytes = std::fs::read(&path).with_context(|| format!("{} が読めない", path.display()))?;
    let state = plugin_presets::obxf::obxf_state_blob(&bytes)?;
    super::patch_state::load_plugin_state(instance, &state)
        .with_context(|| format!("OB-Xf の音色を読めない: {patch_path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surge_and_obxf_cannot_receive_each_others_fxp() {
        assert!(ensure_state_file_owner(OBXF_PLUGIN_ID, OBXF_PATCH_TAG).is_ok());
        assert!(ensure_state_file_owner(SURGE_XT_PLUGIN_ID, "").is_ok());
        assert!(ensure_state_file_owner(OBXF_PLUGIN_ID, "").is_err());
        assert!(ensure_state_file_owner(SURGE_XT_PLUGIN_ID, OBXF_PATCH_TAG).is_err());
    }
}
