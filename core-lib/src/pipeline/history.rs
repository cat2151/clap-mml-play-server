//! `patch_history.txt` への追記。

use anyhow::Result;

use crate::patch_list::to_relative;
use crate::CoreConfig;

use mmlabc_to_smf::mml_preprocessor;

/// patch_history.txt に「JSON、MML」形式で追記する。
pub(super) fn append_history(mml: &str, patch: &Option<String>, cfg: &CoreConfig) -> Result<()> {
    let patch_rel = match patch {
        Some(abs) => {
            if let Some(ref base) = cfg.patches_dir {
                to_relative(base, std::path::Path::new(abs))
            } else {
                abs.clone()
            }
        }
        None => "(none)".to_string(),
    };

    // JSON部分を除いたMML本文（先頭JSONがあれば除去済みのものを使う）
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let mml_body = preprocessed.remaining_mml.trim().to_string();

    let json = format!(
        "{{\"Surge XT patch\": \"{}\"}}",
        patch_rel.replace('\\', "/")
    );
    let line = format!("{} {}\n", json, mml_body);

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
