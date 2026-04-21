use anyhow::Result;

/// config_local_dir()/clap-mml-render-tui/ ディレクトリを作成し、パスを返す。
/// `phrase/` および `daw/` サブディレクトリの親ディレクトリとしても使用される。
/// テスト時は環境変数 `CMRT_BASE_DIR` でベースパスを上書きできる。
pub fn ensure_cmrt_dir() -> Result<std::path::PathBuf> {
    let dir = cmrt_base_dir()?.join("clap-mml-render-tui");
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow::anyhow!("clap-mml-render-tui/ ディレクトリの作成に失敗: {}", e))?;
    Ok(dir)
}

/// config_local_dir()/clap-mml-render-tui/phrase/ ディレクトリを作成し、パスを返す。
/// フレーズモード（非DAWモード）の出力ファイル（output.mid, output.wav）を格納する。
/// テスト時は環境変数 `CMRT_BASE_DIR` でベースパスを上書きできる。
pub fn ensure_phrase_dir() -> Result<std::path::PathBuf> {
    let dir = cmrt_base_dir()?.join("clap-mml-render-tui").join("phrase");
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow::anyhow!("phrase/ ディレクトリの作成に失敗: {}", e))?;
    Ok(dir)
}

/// config_local_dir()/clap-mml-render-tui/daw/ ディレクトリを作成し、パスを返す。
/// DAWモードの出力ファイル（daw_cache.mid, daw_cache.wav, per-track WAV 等）を格納する。
/// テスト時は環境変数 `CMRT_BASE_DIR` でベースパスを上書きできる。
pub fn ensure_daw_dir() -> Result<std::path::PathBuf> {
    let dir = cmrt_base_dir()?.join("clap-mml-render-tui").join("daw");
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow::anyhow!("daw/ ディレクトリの作成に失敗: {}", e))?;
    Ok(dir)
}

/// `clap-mml-render-tui/` の親ディレクトリを返す。
/// 環境変数 `CMRT_BASE_DIR` が設定されていればそれを使い、なければ `dirs::config_local_dir()` を使う。
/// テストでは `CMRT_BASE_DIR` に一時ディレクトリを設定することで実際の設定ディレクトリへの書き込みを避ける。
/// 戻り値: 親ディレクトリのパス（`PathBuf`）。設定ディレクトリが取得できない場合はエラーを返す。
fn cmrt_base_dir() -> Result<std::path::PathBuf> {
    if let Some(base) = std::env::var_os("CMRT_BASE_DIR") {
        return Ok(std::path::PathBuf::from(base));
    }
    dirs::config_local_dir()
        .ok_or_else(|| anyhow::anyhow!("システム設定ディレクトリが取得できません"))
}
