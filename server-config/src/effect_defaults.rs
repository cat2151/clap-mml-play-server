//! audio effect plugin（TONE3000 / Surge XT Effects / Dragonfly Reverb）の標準インストール先と factory preset 置き場。
//!
//! instrument と違い config.toml には項目を持たず、組み込みの既定値だけで探す。
//! 実在しなければ effect catalog に載らない（[`crate::default_vaporizer2_plugin_path`] と
//! 同じく安全側に倒れる）。

use std::path::PathBuf;

/// OS ごとのデフォルト TONE3000 パス。
#[cfg(target_os = "windows")]
pub fn default_tone3000_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\TONE3000.clap"
}

#[cfg(target_os = "macos")]
pub fn default_tone3000_plugin_path() -> &'static str {
    "/Library/Audio/Plug-Ins/CLAP/TONE3000.clap"
}

#[cfg(target_os = "linux")]
pub fn default_tone3000_plugin_path() -> &'static str {
    "/usr/lib/clap/TONE3000.clap"
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_tone3000_plugin_path() -> &'static str {
    ""
}

/// OS ごとのデフォルト Surge XT Effects パス。Surge XT 本体と同じディレクトリに入る。
#[cfg(target_os = "windows")]
pub fn default_surge_fx_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT Effects.clap"
}

#[cfg(target_os = "macos")]
pub fn default_surge_fx_plugin_path() -> &'static str {
    "/Library/Audio/Plug-Ins/CLAP/Surge XT Effects.clap"
}

#[cfg(target_os = "linux")]
pub fn default_surge_fx_plugin_path() -> &'static str {
    "/usr/lib/clap/Surge XT Effects.clap"
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_surge_fx_plugin_path() -> &'static str {
    ""
}

/// TONE3000 の preset 置き場（factory preset は `Factory/` 配下）。
///
/// 置き場が確認できていない OS では `None` を返し、catalog に載せない。
#[cfg(target_os = "windows")]
pub fn default_tone3000_preset_root() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|dir| PathBuf::from(dir).join("TONE3000").join("Presets"))
}

#[cfg(not(target_os = "windows"))]
pub fn default_tone3000_preset_root() -> Option<PathBuf> {
    None
}

/// Surge XT Effects の factory preset 置き場 `fx_presets`。Surge XT のデータディレクトリ配下。
#[cfg(target_os = "windows")]
pub fn default_surge_fx_preset_root() -> Option<PathBuf> {
    std::env::var_os("ProgramData")
        .map(|dir| PathBuf::from(dir).join("Surge XT").join("fx_presets"))
}

#[cfg(target_os = "macos")]
pub fn default_surge_fx_preset_root() -> Option<PathBuf> {
    Some(PathBuf::from(
        "/Library/Application Support/Surge XT/fx_presets",
    ))
}

#[cfg(target_os = "linux")]
pub fn default_surge_fx_preset_root() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("surge-data").join("fx_presets"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_surge_fx_preset_root() -> Option<PathBuf> {
    None
}

/// Dragonfly Reverb の plugin 1 つのパス。`file_stem` は `DragonflyHallReverb` など。
/// preset は plugin 本体に組み込まれていて、置き場は無い。
#[cfg(target_os = "windows")]
pub fn default_dragonfly_plugin_path(file_stem: &str) -> PathBuf {
    PathBuf::from(r"C:\Program Files\Common Files\CLAP\dragonfly-reverb")
        .join(format!("{file_stem}.clap"))
}

#[cfg(target_os = "macos")]
pub fn default_dragonfly_plugin_path(file_stem: &str) -> PathBuf {
    PathBuf::from("/Library/Audio/Plug-Ins/CLAP").join(format!("{file_stem}.clap"))
}

#[cfg(target_os = "linux")]
pub fn default_dragonfly_plugin_path(file_stem: &str) -> PathBuf {
    PathBuf::from("/usr/lib/clap").join(format!("{file_stem}.clap"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_dragonfly_plugin_path(_file_stem: &str) -> PathBuf {
    PathBuf::new()
}
