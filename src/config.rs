use serde::Deserialize;
use std::path::PathBuf;

const DEFAULT_CONFIG: &str = include_str!("../config.toml");

#[derive(Deserialize, Debug)]
pub struct Config {
    pub plugin_path: String,
    #[allow(dead_code)]
    pub input_midi: String,
    pub output_midi: String,
    pub output_wav: String,
    pub sample_rate: f64,
    pub buffer_size: usize,
    /// オプション: .fxp パッチファイルのパス。指定しない場合は Init Saw のまま。
    pub patch_path: Option<String>,
    /// ファクトリパッチのルートディレクトリ
    pub patches_dir: Option<String>,
    /// true のとき演奏ごとにランダムなパッチを選ぶ（デフォルト true）
    #[serde(default = "default_random_patch")]
    pub random_patch: bool,
}

fn default_random_patch() -> bool {
    true
}

/// OS 標準の設定ディレクトリ内の config.toml パスを返す。
/// - Windows: %APPDATA%\cmrt\config.toml
/// - Linux:   ~/.config/cmrt/config.toml
/// - macOS:   ~/Library/Application Support/cmrt/config.toml
/// システムの設定ディレクトリが取得できない場合は `None` を返す。
pub fn config_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("cmrt").join("config.toml"))
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        // カレントディレクトリの config.toml を最優先（後方互換性・Windows 向け）
        let cwd_path = std::path::Path::new("config.toml");
        if cwd_path.exists() {
            let text = std::fs::read_to_string(cwd_path)
                .map_err(|e| anyhow::anyhow!("config.toml が読めない ({}): {}", cwd_path.display(), e))?;
            return toml::from_str(&text)
                .map_err(|e| anyhow::anyhow!("config.toml のパースに失敗 ({}): {}", cwd_path.display(), e));
        }

        let path = config_file_path().ok_or_else(|| {
            anyhow::anyhow!(
                "システムの設定ディレクトリが取得できません。HOME 環境変数などを確認してください。"
            )
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // create_new で排他的に作成することでレースコンディションを回避する。
        // AlreadyExists は既にファイルがある正常ケースなので無視する。
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write as _;
                match file.write_all(DEFAULT_CONFIG.as_bytes()) {
                    Ok(_) => {
                        eprintln!(
                            "デフォルトの config.toml を作成しました: {}",
                            path.display()
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "デフォルト config.toml の書き込みに失敗 ({}):\n--- 書き込もうとした内容 ---\n{}\n--- エラー: {}",
                            path.display(),
                            DEFAULT_CONFIG,
                            e
                        );
                        return Err(anyhow::anyhow!(
                            "デフォルト config.toml の書き込みに失敗 ({}): {}",
                            path.display(),
                            e
                        ));
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                eprintln!(
                    "config.toml の作成に失敗 ({}):\n--- 書き込もうとした内容 ---\n{}\n--- エラー: {}",
                    path.display(),
                    DEFAULT_CONFIG,
                    e
                );
                return Err(anyhow::anyhow!(
                    "config.toml の作成に失敗 ({}): {}",
                    path.display(),
                    e
                ));
            }
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("config.toml が読めない ({}): {}", path.display(), e))?;
        toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("config.toml のパースに失敗 ({}): {}", path.display(), e))
    }
}
