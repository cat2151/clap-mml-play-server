//! history.json によるセッション状態の保存・復元。
//!
//! voicevox-playground-tui に倣い、終了時に現在行番号を保存し、
//! 起動時に復元する。

use std::path::PathBuf;

use anyhow::Result;

/// 起動・終了で保存・復元するセッション状態。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SessionState {
    /// 現在行番号（0始まり）。
    pub cursor: usize,
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
pub fn load_session_state() -> SessionState {
    let Some(path) = session_state_path() else {
        return SessionState::default();
    };
    if !path.exists() {
        return SessionState::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_default_cursor_is_zero() {
        let state = SessionState::default();
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn session_state_serialize_deserialize() {
        let state = SessionState { cursor: 42 };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.cursor, 42);
    }

    #[test]
    fn session_state_serialize_deserialize_zero() {
        let state = SessionState { cursor: 0 };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.cursor, 0);
    }

    #[test]
    fn session_state_json_from_invalid_returns_default() {
        // 不正なJSONはデフォルト値を返す
        let result: SessionState = serde_json::from_str("not json")
            .unwrap_or_default();
        assert_eq!(result.cursor, 0);
    }

    #[test]
    fn session_state_json_missing_field_returns_default() {
        // cursor フィールドがない場合はデフォルト値を返す
        let result: SessionState = serde_json::from_str("{}")
            .unwrap_or_default();
        assert_eq!(result.cursor, 0);
    }

    #[test]
    fn save_and_load_session_state_roundtrip() {
        // 実ユーザーデータディレクトリに影響しないよう、一時ファイルに直接書き込んで
        // JSON シリアライズ/デシリアライズの往復を検証する
        let tmp_path = std::env::temp_dir().join("cmrt_test_history_roundtrip.json");

        let state = SessionState { cursor: 7 };
        let json = serde_json::to_string_pretty(&state).unwrap();
        std::fs::write(&tmp_path, &json).unwrap();

        let read_back = std::fs::read_to_string(&tmp_path).unwrap();
        let loaded: SessionState = serde_json::from_str(&read_back).unwrap();
        std::fs::remove_file(&tmp_path).ok();

        assert_eq!(loaded.cursor, 7);
    }
}
