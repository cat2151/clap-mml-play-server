//! MML 先頭 JSON・config・ランダム選択から、render に使う音色（wire）を決める。

use anyhow::Result;

use crate::patch_list::collect_patches;
use crate::patch_tag::{
    default_owner_wire, display_from_wire, parse_patch_wire, patch_from_embedded_json,
};
use crate::CoreConfig;

use mmlabc_to_smf::mml_preprocessor;

pub(super) fn resolve_effective_patch(
    embedded_json: Option<&str>,
    cfg: &CoreConfig,
    allow_random_patch: bool,
) -> Result<Option<String>> {
    if let Some(patch) = extract_patch_from_json(embedded_json, cfg)? {
        return Ok(Some(patch));
    }
    if allow_random_patch {
        return pick_random_patch(cfg);
    }
    Ok(cfg.patch_path.clone())
}

pub(super) fn patch_display_for_render(effective_patch: Option<&str>, cfg: &CoreConfig) -> String {
    let Some(patch) = effective_patch else {
        return "(Init Saw)".to_string();
    };
    match parse_patch_wire(patch) {
        Ok(wire) => {
            let display = cfg.patch_base.display(std::path::Path::new(wire.path));
            display_from_wire(&crate::PatchWire {
                path: &display,
                ..wire
            })
        }
        Err(_) => patch.to_string(),
    }
}

/// MML 先頭 JSON が指す音色の wire を、**解決せずそのまま**返す。旧キーはエラー。
///
/// 「この MML をどのプラグインへ渡すか」の判別はこの未解決の文字列だけで足りる
/// （[`crate::kind_for_patch`]）。解決の基点（`CoreConfig.patch_base`）は
/// プラグインごとに違うので、プラグインを決める前には選べない。
pub fn embedded_patch_ref(mml: &str) -> Result<Option<String>> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let Some(value) = parse_embedded_json(preprocessed.embedded_json.as_deref()) else {
        return Ok(None);
    };
    Ok(patch_from_embedded_json(&value)
        .map_err(anyhow::Error::msg)?
        .map(str::to_string))
}

fn parse_embedded_json(json_str: Option<&str>) -> Option<serde_json::Value> {
    serde_json::from_str(json_str?).ok()
}

/// MML 先頭 JSON の音色の wire を取り出し、パス部分を絶対パスに変換する。
pub(super) fn extract_patch_from_json(
    json_str: Option<&str>,
    cfg: &CoreConfig,
) -> Result<Option<String>> {
    let Some(value) = parse_embedded_json(json_str) else {
        return Ok(None);
    };
    let Some(patch) = patch_from_embedded_json(&value).map_err(anyhow::Error::msg)? else {
        return Ok(None);
    };
    let wire = parse_patch_wire(patch)?;
    // 基点があれば絶対パスに変換、なければそのまま
    if cfg.patch_base == crate::PatchBase::None {
        return Ok(Some(patch.to_string()));
    }
    let path = cfg
        .patch_base
        .resolve(&wire.path.replace('/', std::path::MAIN_SEPARATOR_STR));
    Ok(Some(wire.with_path(&path)))
}

/// 基点のディレクトリからランダムに1つ選んで、既定所有者の wire（絶対パス）を返す。
fn pick_random_patch(cfg: &CoreConfig) -> Result<Option<String>> {
    let Some(dir) = cfg.patch_base.scan_dir() else {
        return Ok(None);
    };
    let patches = collect_patches(dir)?;
    if patches.is_empty() {
        return Ok(None);
    }
    // 簡易乱数: 現在時刻のナノ秒を使う
    let idx = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0) as usize;
        ns % patches.len()
    };
    Ok(Some(default_owner_wire(&patches[idx].to_string_lossy())))
}
