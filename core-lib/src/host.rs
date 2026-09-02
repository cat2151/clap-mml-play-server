//! 最小限の CLAP ホスト実装
//!
//! clack-host の公式 README の HostHandlers モデルに従う。

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use clack_extensions::preset_discovery::{
    preset_data::Location, HostPresetLoad, HostPresetLoadImpl,
};
use clack_host::prelude::*;
use std::ffi::CStr;

use crate::logging::emit_diagnostic;

// -----------------------------------------------------------------------
// HostShared – スレッド間で共有される状態（今は空）
// -----------------------------------------------------------------------
#[derive(Default)]
pub struct MidiRenderHostShared {
    callback_requested: AtomicBool,
}

impl MidiRenderHostShared {
    /// プラグインが要求した main-thread callback を一度だけ取り出す。
    pub(crate) fn take_callback_request(&self) -> bool {
        self.callback_requested.swap(false, Ordering::AcqRel)
    }
}

impl<'a> SharedHandler<'a> for MidiRenderHostShared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {
        self.callback_requested.store(true, Ordering::Release);
    }
}

// -----------------------------------------------------------------------
// HostHandlers – ホスト実装のルートトレイト
// -----------------------------------------------------------------------
pub struct MidiRenderHost;

pub struct MidiRenderHostMainThread;

impl MainThreadHandler<'_> for MidiRenderHostMainThread {}

impl HostPresetLoadImpl for MidiRenderHostMainThread {
    fn on_error(
        &self,
        location: Location,
        _load_key: Option<&CStr>,
        os_error: i32,
        message: Option<&CStr>,
    ) {
        emit_diagnostic(format!(
            "CLAP preset-load error: location={location:?} os_error={os_error} detail={}",
            message
                .map(|message| message.to_string_lossy())
                .unwrap_or_default()
        ));
    }

    fn loaded(&self, _location: Location, _load_key: Option<&CStr>) {}
}

impl HostHandlers for MidiRenderHost {
    type Shared<'a> = MidiRenderHostShared;
    type MainThread<'a> = MidiRenderHostMainThread;
    type AudioProcessor<'a> = (); // オーディオスレッド処理も今回不要

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder.register::<HostPresetLoad>();
    }
}

// -----------------------------------------------------------------------
// ヘルパー: プラグインエントリをロードして返す
// -----------------------------------------------------------------------
/// ロード済みの CLAP entry。生成したインスタンスが clone を保持するので、
/// これを drop しても生きているインスタンスの足元は崩れない。
///
/// clack の型をそのまま出しているのは、呼び出し側（サーバー）が entry を
/// 保持して複数のプラグインを使い分けるために型名が要るため。
pub use clack_host::prelude::PluginEntry;

/// 組み込みプラグインを指す擬似 `plugin_path` の頭。後ろに CLAP ID を書く
/// （例: `builtin:org.cat2151.cmrt.cache-player`）。
///
/// ディスク上のパスと同じ場所に置けるようにしてあるのは、`PluginKind` から下流
/// （`create_live_renderers` / 予備プールの `builder`）が「entry をどう読むか」を
/// 知らずに済ませるため。分岐は [`load_entry`] の 1 か所だけで閉じる。
pub const BUILTIN_PLUGIN_PATH_PREFIX: &str = "builtin:";

/// 組み込みプラグインの擬似 `plugin_path` を組み立てる。
pub fn builtin_plugin_path(plugin_id: &str) -> String {
    format!("{BUILTIN_PLUGIN_PATH_PREFIX}{plugin_id}")
}

pub fn load_entry(path: &str) -> Result<PluginEntry> {
    if let Some(plugin_id) = path.strip_prefix(BUILTIN_PLUGIN_PATH_PREFIX) {
        return load_builtin_entry_by_id(plugin_id);
    }
    // SAFETY: CLAP プラグインのロードは unsafe を伴う
    let entry = unsafe {
        PluginEntry::load(path)
            .map_err(|e| anyhow::anyhow!("プラグインのロードに失敗 ({}): {:?}", path, e))?
    };
    Ok(entry)
}

/// このバイナリへ静的リンクされた CLAP entry を読む。
///
/// `PluginEntry::load`（DLL を読む unsafe な方）と**返り値が同じ**なので、
/// descriptor 選択から先は組み込みかどうかを気にしなくてよい。clack が
/// 「this method is completely safe」と明記している経路なので、`.clap` ファイルを
/// 作るより安全でもある。
pub fn load_builtin_entry<E: clack_plugin::entry::Entry>() -> Result<PluginEntry> {
    // 空の CStr は「バンドルのパスを持たない」という意味。組み込み entry は
    // ディスク上の位置を持たないので、これが正しい渡し方になる。
    PluginEntry::load_from_clack::<E>(c"")
        .map_err(|e| anyhow::anyhow!("組み込みプラグインのロードに失敗: {:?}", e))
}

/// CLAP ID で組み込み entry を引く。知らない ID はエラー。
fn load_builtin_entry_by_id(plugin_id: &str) -> Result<PluginEntry> {
    match plugin_id {
        cmrt_cache_player::CACHE_PLAYER_PLUGIN_ID => {
            load_builtin_entry::<cmrt_cache_player::CachePlayerEntry>()
        }
        other => anyhow::bail!("組み込みプラグインに '{other}' は無い"),
    }
}
