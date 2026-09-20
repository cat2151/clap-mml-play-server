//! instrument の後段に挿す effect plugin の catalog と、ロード済み `PluginEntry` の表。
//!
//! # なぜ instrument の entry 表と別か
//! instrument の entry は「MML が指す音色」で引き分ける（呼び出し側が持つ entry 表。
//! 例えば clap-mml-render-tui の `PluginEntries`）が、effect は MML 先頭 JSON の chain
//! 要素のキーで決まり、catalog も preset の置き場も別物（[`crate::AudioEffectCatalog`]）。
//! 混在させると「音色無指定なら先頭」の規則が effect にも効いてしまう。
//!
//! # ロードのタイミング
//! catalog の走査（preset ファイルの読み取り）も effect plugin の DLL ロードも、最初に
//! 要るときまで遅らせる。呼び出し側の起動時に effect が無い環境で待たされないためで、
//! 一度読んだものは保持してレンダリングのたびに DLL を読み直さない。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use anyhow::Result;
use clack_host::prelude::PluginEntry;

use crate::{AudioEffectCatalog, AudioEffectPluginInfo, PluginKey, RenderEffects};

/// effect の catalog と entry を、レンダリング経路のあいだで共有する手元。
#[derive(Clone, Default)]
pub struct EffectPlugins {
    inner: Arc<EffectPluginsInner>,
}

#[derive(Default)]
enum EffectPluginsInner {
    /// chain 付きの MML を受け付けない経路（テスト）。
    #[default]
    Disabled,
    Enabled {
        catalog: OnceLock<AudioEffectCatalog>,
        entries: Mutex<BTreeMap<PluginKey, PluginEntry>>,
    },
}

impl EffectPlugins {
    /// 組み込み既定パスから探す。走査は最初に catalog が要るときまで遅らせる。
    pub fn discover() -> Self {
        Self::enabled(OnceLock::new())
    }

    /// 組み立て済みの catalog から作る。catalog を手で並べたいテスト用でもある。
    pub fn with_catalog(catalog: AudioEffectCatalog) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(catalog);
        Self::enabled(cell)
    }

    /// chain 付きの MML を受け付けない経路（テスト）用。
    pub fn none() -> Self {
        Self::default()
    }

    fn enabled(catalog: OnceLock<AudioEffectCatalog>) -> Self {
        Self {
            inner: Arc::new(EffectPluginsInner::Enabled {
                catalog,
                entries: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// effect の catalog。受け付けない経路では `None`。初回は走査を行う。
    pub fn catalog(&self) -> Option<&AudioEffectCatalog> {
        match self.inner.as_ref() {
            EffectPluginsInner::Disabled => None,
            EffectPluginsInner::Enabled { catalog, .. } => {
                Some(catalog.get_or_init(AudioEffectCatalog::discover))
            }
        }
    }

    /// この経路が chain 付きの MML をレンダリングできるか。
    pub fn is_available(&self) -> bool {
        !matches!(self.inner.as_ref(), EffectPluginsInner::Disabled)
    }

    /// レンダリング 1 回ぶんの `RenderEffects` を組んで `render` に渡す。
    ///
    /// `RenderEffects` は catalog と entry loader への参照を持つだけなので、
    /// この呼び出しの中でしか使えない。
    pub fn with_render_effects<R>(&self, render: impl FnOnce(RenderEffects<'_>) -> R) -> R {
        let Some(catalog) = self.catalog() else {
            return render(RenderEffects::unsupported());
        };
        let load_entry = |plugin: &AudioEffectPluginInfo| self.entry(plugin);
        render(RenderEffects::new(catalog, &load_entry))
    }

    /// effect plugin の entry。初回にロードし、以後は保持したものを返す。
    ///
    /// ロード中も表の lock を握るので、同じ effect を初めて要る worker が並ぶと
    /// 後続は lock 待ちになる。待った時間とロードにかかった時間を stderr へ残し、
    /// 並列 render が遅いときにここで詰まっているかを外から判定できるようにする。
    fn entry(&self, plugin: &AudioEffectPluginInfo) -> Result<PluginEntry> {
        let EffectPluginsInner::Enabled { entries, .. } = self.inner.as_ref() else {
            anyhow::bail!("この render 経路は effect plugin をロードしない");
        };
        let lock_requested = Instant::now();
        let mut entries = entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let lock_wait_ms = lock_requested.elapsed().as_millis();
        if let Some(entry) = entries.get(&plugin.key) {
            log_entry_event(plugin, "cached", lock_wait_ms, None);
            return Ok(entry.clone());
        }
        log_entry_event(plugin, "load-start", lock_wait_ms, None);
        let load_started = Instant::now();
        let loaded = crate::load_entry(&plugin.plugin_path);
        let load_ms = load_started.elapsed().as_millis();
        let entry = match loaded {
            Ok(entry) => entry,
            Err(error) => {
                log_entry_event(plugin, "load-failed", lock_wait_ms, Some(load_ms));
                return Err(error);
            }
        };
        log_entry_event(plugin, "load-end", lock_wait_ms, Some(load_ms));
        entries.insert(plugin.key.clone(), entry.clone());
        Ok(entry)
    }
}

fn log_entry_event(
    plugin: &AudioEffectPluginInfo,
    event: &str,
    lock_wait_ms: u128,
    load_ms: Option<u128>,
) {
    let load_ms = load_ms
        .map(|ms| format!(" load_ms={ms}"))
        .unwrap_or_default();
    eprintln!(
        "cmrt-effect-entry: plugin={} event={event} lock_wait_ms={lock_wait_ms}{load_ms} thread={:?}",
        plugin.name,
        std::thread::current().id()
    );
}

#[cfg(test)]
mod tests;
