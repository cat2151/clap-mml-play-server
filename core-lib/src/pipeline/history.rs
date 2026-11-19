//! `patch_history.txt` への追記。

use anyhow::Result;

use crate::patch_tag::{parse_patch_wire, PATCH_JSON_KEY};
use crate::CoreConfig;

use mmlabc_to_smf::mml_preprocessor;

/// patch_history.txt に「JSON、MML」形式で追記する。
pub(super) fn append_history(mml: &str, patch: &Option<String>, cfg: &CoreConfig) -> Result<()> {
    let line = history_line(mml, patch, cfg)?;

    use std::io::Write;
    let Some(path) =
        dirs::config_local_dir().map(|d| d.join("clap-mml-render-tui").join("patch_history.txt"))
    else {
        return Ok(()); // ディレクトリが取得できない場合はスキップ
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| anyhow::anyhow!("patch_history.txt のディレクトリ作成失敗: {}", e))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| anyhow::anyhow!("patch_history.txt を開けない: {}", e))?;
    file.write_all(line.as_bytes())
        .map_err(|e| anyhow::anyhow!("patch_history.txt への書き込み失敗: {}", e))?;
    Ok(())
}

/// 履歴 1 行。音色は基点からの相対の wire で書く。
pub(super) fn history_line(mml: &str, patch: &Option<String>, cfg: &CoreConfig) -> Result<String> {
    let patch_wire = match patch {
        Some(patch) => {
            let wire = parse_patch_wire(patch)?;
            let display = cfg.patch_base.display(std::path::Path::new(wire.path));
            wire.with_path(&display.replace('\\', "/"))
        }
        None => "(none)".to_string(),
    };

    // JSON部分を除いたMML本文（先頭JSONがあれば除去済みのものを使う）
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let mml_body = preprocessed.remaining_mml.trim().to_string();

    let json = format!(
        "{{\"{PATCH_JSON_KEY}\": {}}}",
        serde_json::to_string(&patch_wire)?
    );
    Ok(format!("{} {}\n", json, mml_body))
}
