//! 最小限の CLAP ホスト実装
//!
//! clack-host の公式 README の HostHandlers モデルに従う。

use anyhow::Result;
use clack_host::prelude::*;

// -----------------------------------------------------------------------
// HostShared – スレッド間で共有される状態（今は空）
// -----------------------------------------------------------------------
pub struct MidiRenderHostShared;

impl<'a> SharedHandler<'a> for MidiRenderHostShared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {}
}

// -----------------------------------------------------------------------
// HostHandlers – ホスト実装のルートトレイト
// -----------------------------------------------------------------------
pub struct MidiRenderHost;

impl HostHandlers for MidiRenderHost {
    type Shared<'a> = MidiRenderHostShared;
    type MainThread<'a> = ();      // メインスレッド処理は今回不要
    type AudioProcessor<'a> = (); // オーディオスレッド処理も今回不要
}

// -----------------------------------------------------------------------
// ヘルパー: プラグインエントリをロードして返す
// -----------------------------------------------------------------------
pub fn load_entry(path: &str) -> Result<PluginEntry> {
    // SAFETY: CLAP プラグインのロードは unsafe を伴う
    let entry = unsafe {
        PluginEntry::load(path)
            .map_err(|e| anyhow::anyhow!("プラグインのロードに失敗 ({}): {:?}", path, e))?
    };
    Ok(entry)
}
