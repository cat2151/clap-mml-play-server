//! `list-effect-presets` サブコマンド。effect catalog を組み込み既定パスから組んで出す。
//!
//! サーバーは起動せず、plugin も読まない（preset ファイルの走査だけ）。TUI が
//! overlay に出す一覧と同じ catalog なので、件数と各行の突き合わせに使える。

use std::io::Write as _;

use anyhow::Result;
use cmrt_core::AudioEffectCatalog;
use serde::Serialize;

#[derive(Serialize)]
struct PluginReport<'a> {
    name: &'a str,
    plugin_id: &'a str,
    plugin_path: &'a str,
    json_key: &'a str,
    preset_root: String,
    preset_count: usize,
}

#[derive(Serialize)]
struct PresetReport<'a> {
    display: &'a str,
    json_key: &'a str,
    value: &'a str,
    path: String,
}

#[derive(Serialize)]
struct CatalogReport<'a> {
    plugins: Vec<PluginReport<'a>>,
    presets: Vec<PresetReport<'a>>,
    skipped: &'a [String],
}

pub(crate) fn list_effect_presets(json: bool) -> Result<()> {
    let catalog = AudioEffectCatalog::scan(cmrt_core::builtin_effect_plugins());
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if json {
        serde_json::to_writer_pretty(&mut out, &report(&catalog))?;
        writeln!(out)?;
        return Ok(());
    }
    for plugin in catalog.plugins() {
        let count = catalog
            .presets()
            .iter()
            .filter(|preset| preset.plugin == plugin.key)
            .count();
        writeln!(
            out,
            "{}: {count} presets  key={:?}  plugin={}  root={}",
            plugin.name,
            plugin.json_key,
            plugin.plugin_path,
            plugin.preset_root.display()
        )?;
    }
    for preset in catalog.presets() {
        writeln!(out, "{}\t{}", preset.display, preset.json_element())?;
    }
    for line in catalog.skipped() {
        writeln!(out, "skipped: {line}")?;
    }
    writeln!(
        out,
        "total: {} plugins, {} presets, {} skipped",
        catalog.plugins().len(),
        catalog.presets().len(),
        catalog.skipped().len()
    )?;
    Ok(())
}

fn report(catalog: &AudioEffectCatalog) -> CatalogReport<'_> {
    CatalogReport {
        plugins: catalog
            .plugins()
            .iter()
            .map(|plugin| PluginReport {
                name: &plugin.name,
                plugin_id: &plugin.plugin_id,
                plugin_path: &plugin.plugin_path,
                json_key: &plugin.json_key,
                preset_root: plugin.preset_root.display().to_string(),
                preset_count: catalog
                    .presets()
                    .iter()
                    .filter(|preset| preset.plugin == plugin.key)
                    .count(),
            })
            .collect(),
        presets: catalog
            .presets()
            .iter()
            .map(|preset| PresetReport {
                display: &preset.display,
                json_key: &preset.json_key,
                value: &preset.value,
                path: preset.path.display().to_string(),
            })
            .collect(),
        skipped: catalog.skipped(),
    }
}
