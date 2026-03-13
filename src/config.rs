use serde::Deserialize;
use std::path::PathBuf;

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

/// OS ごとのデフォルト plugin_path を返す。
#[cfg(target_os = "windows")]
fn default_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap"
}

#[cfg(target_os = "macos")]
fn default_plugin_path() -> &'static str {
    "/Library/Audio/Plug-Ins/CLAP/Surge XT.clap"
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn default_plugin_path() -> &'static str {
    "/usr/lib/clap/Surge XT.clap"
}

/// OS に応じたデフォルトの config.toml 内容を生成する。
fn default_config_content() -> String {
    let plugin_path = default_plugin_path();
    format!(
        r#"# clap-midi-render config
#
# 【必須】plugin_path にお使いの CLAP プラグインのパスを設定してください。
# 例 (Windows): plugin_path = 'C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap'
# 例 (Linux):   plugin_path = '/usr/lib/clap/Surge XT.clap'
# 例 (macOS):   plugin_path = '/Library/Audio/Plug-Ins/CLAP/Surge XT.clap'
plugin_path = '{plugin_path}'

input_midi  = "input.mid"
output_midi = "output.mid"
output_wav  = "output.wav"
sample_rate = 44100
buffer_size = 512

# 【省略可】ファクトリパッチのルートディレクトリ
# 例 (Windows): patches_dir = 'C:\ProgramData\Surge XT\patches_factory'
# 例 (Linux):   patches_dir = '/home/user/.local/share/surge-data/patches_factory'
# patches_dir = ""

# true: 演奏ごとにランダムなパッチを選ぶ（デフォルト true）
# false: 下の patch_path を使う
random_patch = true

# 【省略可】random_patch = false のときに使う音色
# patch_path = ""
"#
    )
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
        let content = default_config_content();
        // create_new で排他的に作成することでレースコンディションを回避する。
        // AlreadyExists は既にファイルがある正常ケースなので無視する。
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write as _;
                match file.write_all(content.as_bytes()) {
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
                            content,
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
                    content,
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
