//! history.json によるセッション状態の保存・復元。
//!
//! voicevox-playground-tui に倣い、終了時に現在行番号と編集行を保存し、
//! 起動時に復元する。

use std::path::PathBuf;

use anyhow::Result;

fn default_lines() -> Vec<String> {
    vec!["cde".to_string()]
}

/// 起動・終了で保存・復元するセッション状態。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionState {
    /// 現在行番号（0始まり）。
    #[serde(default)]
    pub cursor: usize,
    /// 編集行リスト。
    #[serde(default = "default_lines")]
    pub lines: Vec<String>,
    /// 終了時に DAW モードだったかどうか。起動時に復元する。
    #[serde(default)]
    pub is_daw_mode: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            cursor: 0,
            lines: default_lines(),
            is_daw_mode: false,
        }
    }
}

/// OS ごとのデータディレクトリ配下の `cmrt` サブディレクトリを返す。
/// config.toml と同じ `cmrt` プレフィックスに揃えることで、ユーザーデータの場所を一貫させる。
/// `dirs::data_local_dir()` が利用できない環境では `None` を返し、保存・復元をスキップする。
fn history_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("cmrt"))
}

fn session_state_path() -> Option<PathBuf> {
    history_dir().map(|d| d.join("history.json"))
}

/// DAW データファイル (`daw.txt`) のパスを返す。
/// `history.json` と同じディレクトリに配置することでユーザーデータの場所を統一する。
/// `dirs::data_local_dir()` が利用できない環境では `None` を返す。
pub fn daw_file_path() -> Option<PathBuf> {
    history_dir().map(|d| d.join("daw.txt"))
}

/// セッション状態（現在行番号）を history.json に保存する。
/// データディレクトリが利用できない場合はベストエフォートでスキップする。
pub fn save_session_state(state: &SessionState) -> Result<()> {
    let Some(path) = session_state_path() else { return Ok(()); };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// history.json からセッション状態を読み込む。
/// ファイルが存在しない場合・データディレクトリが利用できない場合・読み込みに失敗した場合は
/// デフォルト値を返す。
/// `lines` が空の場合（`"lines": []` のような入力）はデフォルト値で補填し、
/// `lines` が常に1行以上という不変条件を保証する。
pub fn load_session_state() -> SessionState {
    let Some(path) = session_state_path() else {
        return SessionState::default();
    };
    if !path.exists() {
        return SessionState::default();
    }
    let mut state: SessionState = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if state.lines.is_empty() {
        state.lines = default_lines();
    }
    state
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
