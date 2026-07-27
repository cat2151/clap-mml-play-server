//! パッチファイル（.fxp）とプラグイン state の入出力。
//!
//! レンダリングループ本体（`render.rs`）から、ファイル形式の知識と
//! CLAP state extension の扱いを切り離している。

use anyhow::Result;
use clack_extensions::state::PluginState;
use clack_host::prelude::*;

use crate::host::MidiRenderHost;

/// .fxp ファイルを clap state として plugin にロードする
///
/// Surge XTの .fxp は VST2 の opaque chunk 形式:
///   Bytes  0-3 : 'CcnK'
///   Bytes  4-7 : byteSize (big-endian)
///   Bytes  8-11: 'FPCh' (opaque chunk preset)
///   Bytes 12-27: version / fxID / fxVersion / numPrograms
///   Bytes 28-31: chunkSize (big-endian)
///   Bytes 32+  : chunk data (== Surge 独自形式: 'sub3' + xml + wavetables)
///
/// CLAP state として渡すべきは chunk data (offset 32 以降) のみ。
pub(super) fn load_patch(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    patch_path: &str,
) -> Result<()> {
    let raw = std::fs::read(patch_path)
        .map_err(|e| anyhow::anyhow!("パッチファイルを読めない '{}': {}", patch_path, e))?;

    // FXP ヘッダを検出して chunk data だけを切り出す
    //
    // Surge XT の .fxp は標準FXPと異なる独自レイアウト:
    //   offset  0- 3: 'CcnK'
    //   offset  4- 7: byteSize (big-endian, Surgeは0埋め)
    //   offset  8-11: 'FPCh'
    //   offset 12-27: version / fxID('cjs3') / fxVersion / numPrograms
    //   offset 28-55: プリセット名等 (28バイト, 独自フィールド)
    //   offset 56-59: chunkSize (big-endian)
    //   offset 60+  : chunk data ('sub3' + xml + wavetables)
    let chunk_data: &[u8] = if raw.len() >= 60 && &raw[0..4] == b"CcnK" && &raw[8..12] == b"FPCh" {
        // 'sub3' が offset 60 にあることを確認
        if &raw[60..64] == b"sub3" {
            let chunk_size = u32::from_be_bytes([raw[56], raw[57], raw[58], raw[59]]) as usize;
            let end = (60 + chunk_size).min(raw.len());
            &raw[60..end]
        } else {
            // 念のため 'sub3' をスキャンして見つける
            let pos = raw.windows(4).position(|w| w == b"sub3").unwrap_or(0);
            &raw[pos..]
        }
    } else {
        // FXP ヘッダなし: そのまま渡す（'sub3' 形式か XML か）
        &raw[..]
    };

    load_plugin_state(plugin_instance, chunk_data)
        .map_err(|e| anyhow::anyhow!("パッチのロードに失敗 ({}): {}", patch_path, e))
}

fn plugin_state_extension(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
) -> Result<PluginState> {
    plugin_instance
        .plugin_handle()
        .get_extension::<PluginState>()
        .ok_or_else(|| anyhow::anyhow!("プラグインが state extension をサポートしていない"))
}

/// 現在のプラグイン state をバイト列としてスナップショットする。
pub(super) fn save_plugin_state(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
) -> Result<Vec<u8>> {
    let state_ext = plugin_state_extension(plugin_instance)?;
    let mut bytes = Vec::new();
    let mut handle = plugin_instance.plugin_handle();
    state_ext
        .save(&mut handle, &mut bytes)
        .map_err(|_| anyhow::anyhow!("プラグイン state の保存に失敗"))?;
    Ok(bytes)
}

/// バイト列の state をプラグインへ流し込む。
pub(super) fn load_plugin_state(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    state: &[u8],
) -> Result<()> {
    let state_ext = plugin_state_extension(plugin_instance)?;
    let mut cursor = std::io::Cursor::new(state);
    let mut handle = plugin_instance.plugin_handle();
    state_ext
        .load(&mut handle, &mut cursor)
        .map_err(|_| anyhow::anyhow!("プラグイン state のロードに失敗"))?;
    Ok(())
}
