//! MML 先頭 JSON・config・ランダム選択から、render に使う音色を決める。

use anyhow::Result;

use crate::patch_list::{collect_patches, to_relative};
use crate::CoreConfig;

use mmlabc_to_smf::mml_preprocessor;

pub(super) fn resolve_effective_patch(
    embedded_json: Option<&str>,
    cfg: &CoreConfig,
    allow_random_patch: bool,
) -> Result<Option<String>> {
    if let Some(patch) = extract_patch_from_json(embedded_json, cfg) {
        return Ok(Some(patch));
    }
    if allow_random_patch {
        return pick_random_patch(cfg);
    }
    Ok(cfg.patch_path.clone())
}

pub(super) fn patch_display_for_render(effective_patch: Option<&str>, cfg: &CoreConfig) -> String {
    match effective_patch {
        Some(abs) => {
            if let Some(ref base) = cfg.patches_dir {
                to_relative(base, std::path::Path::new(abs))
            } else {
                abs.to_string()
            }
        }
        None => "(Init Saw)".to_string(),
    }
}

/// MML 先頭 JSON が指す音色の display 文字列を、**解決せずそのまま**返す。
///
/// 「この MML をどのプラグインへ渡すか」の判別はこの未解決の文字列だけで足りる
/// （[`crate::is_cartridge_patch_path`]）。解決の基点（`CoreConfig.patches_dir`）は
/// プラグインごとに違うので、プラグインを決める前には選べない。
pub fn embedded_patch_ref(mml: &str) -> Option<String> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let value: serde_json::Value =
        serde_json::from_str(preprocessed.embedded_json.as_deref()?).ok()?;
    Some(value.get("Surge XT patch")?.as_str()?.to_string())
}

/// MML先頭JSONから "Surge XT patch" キーの値を取り出し、絶対パスに変換する。
pub(super) fn extract_patch_from_json(json_str: Option<&str>, cfg: &CoreConfig) -> Option<String> {
    let json_str = json_str?;
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let rel = v.get("Surge XT patch")?.as_str()?;
    // patches_dir があれば絶対パスに変換、なければそのまま
    if let Some(ref base) = cfg.patches_dir {
        let abs = std::path::Path::new(base).join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        Some(abs.to_string_lossy().into_owned())
    } else {
        Some(rel.to_string())
    }
}

/// patches_dir からランダムに1つ選んで絶対パスを返す。
fn pick_random_patch(cfg: &CoreConfig) -> Result<Option<String>> {
    let dir = match &cfg.patches_dir {
        Some(d) => d,
        None => return Ok(None),
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
    Ok(Some(patches[idx].to_string_lossy().into_owned()))
}
