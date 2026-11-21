//! catalog 構築時に保存した外部 parameter layout cache の reader。

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

use super::SURGE_FX_PARAM_COUNT;

pub const PARAM_LAYOUT_CACHE_RELATIVE_PATH: &str = "patch-catalog/surge-fx-param-layouts.json";
static LAYOUT_FILE: OnceLock<SurgeFxLayoutFile> = OnceLock::new();

/// storage index (`.srgfx` の `pN`) 順の layout。具体的な表は exe に含めない。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SurgeFxParamLayout {
    pub fx_type: i32,
    pub display_name: String,
    /// GUI の並びを決める offset。0 なら GUI の並びに参加しない。
    pub posy_offset: [i32; SURGE_FX_PARAM_COUNT],
    /// `ct_none` でない（値が流れる）parameter。
    pub active: [bool; SURGE_FX_PARAM_COUNT],
    /// plugin が `clap.params` で名乗る名前（group 名を除く）。
    pub names: [String; SURGE_FX_PARAM_COUNT],
}

pub const PARAM_LAYOUT_GENERATOR_VERSION: u32 = 1;

#[derive(Deserialize, Serialize)]
pub struct SurgeFxLayoutFile {
    pub format_version: u32,
    pub generator_version: u32,
    pub plugin_version: String,
    pub source_tag: String,
    pub source_revision: String,
    pub sst_effects_revision: String,
    pub layouts: Vec<SurgeFxParamLayout>,
}

pub fn param_layout_cache_path() -> Result<PathBuf> {
    Ok(dirs::config_local_dir()
        .context("parameter layout cache の保存先を取得できません")?
        .join("clap-mml-render-tui")
        .join(PARAM_LAYOUT_CACHE_RELATIVE_PATH))
}

pub fn decode_layout_file(bytes: &[u8]) -> Result<SurgeFxLayoutFile> {
    let file: SurgeFxLayoutFile =
        serde_json::from_slice(bytes).context("parameter layout JSON が不正です")?;
    ensure!(
        file.format_version == 2 && file.generator_version == PARAM_LAYOUT_GENERATOR_VERSION,
        "parameter layout format version が非対応です"
    );
    ensure!(
        !file.plugin_version.is_empty()
            && !file.source_tag.is_empty()
            && valid_revision(&file.source_revision)
            && valid_revision(&file.sst_effects_revision),
        "parameter layout の生成元がありません"
    );
    ensure!(!file.layouts.is_empty(), "parameter layout が空です");
    let mut ids = HashSet::new();
    for layout in &file.layouts {
        ensure!(
            layout.fx_type > 0 && ids.insert(layout.fx_type),
            "parameter layout の type が不正または重複しています"
        );
        ensure!(
            !layout.display_name.is_empty(),
            "parameter layout の表示名がありません"
        );
    }
    Ok(file)
}

fn valid_revision(revision: &str) -> bool {
    revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn decode_param_layouts(bytes: &[u8]) -> Result<Vec<SurgeFxParamLayout>> {
    Ok(decode_layout_file(bytes)?.layouts)
}

/// A version mismatch must never apply another release's parameter mapping.
pub fn ensure_param_layout_version(plugin_version: &str) -> Result<()> {
    let file = cached_layout_file()?;
    ensure!(file.plugin_version == plugin_version,
        "Surge XT Effects parameter cache の版が不一致 (cache={}, plugin={plugin_version})。cmrt build-patch-catalog-cache で再構築してください", file.plugin_version);
    Ok(())
}

pub fn read_param_layouts(path: &Path) -> Result<Vec<SurgeFxParamLayout>> {
    decode_param_layouts(
        &std::fs::read(path).with_context(|| format!("{} を読めません", path.display()))?,
    )
}

/// 通常利用では cache のみを読み、成功した結果をプロセス内で保持する。
/// 失敗は保持しないため、catalog 構築後には再試行できる。
pub fn param_layouts() -> Result<&'static [SurgeFxParamLayout]> {
    Ok(&cached_layout_file()?.layouts)
}

fn cached_layout_file() -> Result<&'static SurgeFxLayoutFile> {
    if let Some(file) = LAYOUT_FILE.get() {
        return Ok(file);
    }
    let file = std::fs::read(param_layout_cache_path()?).context(
        "Surge XT Effects parameter cache を利用できません。cmrt build-patch-catalog-cache で再構築してください",
    )?;
    let file = decode_layout_file(&file).context(
        "Surge XT Effects parameter cache が不正です。cmrt build-patch-catalog-cache で再構築してください",
    )?;
    let _ = LAYOUT_FILE.set(file);
    Ok(LAYOUT_FILE.get().expect("layout cache initialized"))
}

#[cfg(test)]
mod tests;
