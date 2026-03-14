use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Debug, Clone)]
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
/// 既知 OS でない場合は空文字を返す（ユーザーに設定を促す）。
#[cfg(target_os = "windows")]
fn default_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap"
}

#[cfg(target_os = "macos")]
fn default_plugin_path() -> &'static str {
    "/Library/Audio/Plug-Ins/CLAP/Surge XT.clap"
}

#[cfg(target_os = "linux")]
fn default_plugin_path() -> &'static str {
    "/usr/lib/clap/Surge XT.clap"
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn default_plugin_path() -> &'static str {
    ""
}

/// OS ごとのデフォルト patches_dir を返す。
/// 既知 OS でない場合や取得できない場合は空文字を返す（ユーザーに設定を促す）。
#[cfg(target_os = "windows")]
fn default_patches_dir() -> String {
    r"C:\ProgramData\Surge XT\patches_factory".to_string()
}

#[cfg(target_os = "macos")]
fn default_patches_dir() -> String {
    "/Library/Application Support/Surge XT/patches_factory".to_string()
}

#[cfg(target_os = "linux")]
fn default_patches_dir() -> String {
    dirs::data_dir()
        .map(|d| {
            d.join("surge-data")
                .join("patches_factory")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default()
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn default_patches_dir() -> String {
    String::new()
}

/// OS に応じたデフォルトの config.toml 内容を生成する。
fn default_config_content() -> String {
    let plugin_path = default_plugin_path();
    let plugin_path_line = if plugin_path.is_empty() {
        // 未知の OS: ユーザーに設定を促すためコメントアウト状態で出力する
        "# plugin_path = \"\"  # ← お使いの CLAP プラグインのパスをここに設定してください".to_string()
    } else {
        format!("plugin_path = '{plugin_path}'", plugin_path = plugin_path)
    };
    let patches_dir = default_patches_dir();
    let patches_dir_line = if patches_dir.is_empty() {
        // 未知の OS またはホームディレクトリが取得できない場合
        "# patches_dir = \"\"  # ← ファクトリパッチのルートディレクトリを設定してください".to_string()
    } else {
        format!("patches_dir = '{patches_dir}'", patches_dir = patches_dir)
    };
    format!(
        r#"# cmrt config
#
# 【必須】plugin_path にお使いの CLAP プラグインのパスを設定してください。
# 例 (Windows): plugin_path = 'C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap'
# 例 (Linux):   plugin_path = '/usr/lib/clap/Surge XT.clap'
# 例 (macOS):   plugin_path = '/Library/Audio/Plug-Ins/CLAP/Surge XT.clap'
{plugin_path_line}

input_midi  = "input.mid"
output_midi = "output.mid"
output_wav  = "output.wav"
sample_rate = 44100
buffer_size = 512

# 【省略可】ファクトリパッチのルートディレクトリ（random_patch = true のときに使う）
# 例 (Windows): patches_dir = 'C:\ProgramData\Surge XT\patches_factory'
# 例 (Linux):   patches_dir = '/home/user/.local/share/surge-data/patches_factory'
# 例 (macOS):   patches_dir = '/Library/Application Support/Surge XT/patches_factory'
{patches_dir_line}

# true: 演奏ごとにランダムなパッチを選ぶ（デフォルト true）
# false: 下の patch_path を使う
random_patch = true

# 【省略可】random_patch = false のときに使う音色
# patch_path = ""
"#,
        plugin_path_line = plugin_path_line,
        patches_dir_line = patches_dir_line
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

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
