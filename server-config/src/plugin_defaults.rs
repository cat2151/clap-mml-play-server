//! プラグインの標準インストール先と、そのプラグインの音色置き場の既定値。
//!
//! 「どこにプラグインがあるか」はプラグインをロードする側の知識なので、
//! config を読む crate ではなくこの crate が持つ。組み込みプロファイル
//! （[`crate::builtin_plugin_profiles`]）の値の出どころでもある。

/// OS ごとのデフォルト plugin_path を返す。
/// 既知 OS でない場合は空文字を返す（ユーザーに設定を促す）。
#[cfg(target_os = "windows")]
pub fn default_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap"
}

#[cfg(target_os = "macos")]
pub fn default_plugin_path() -> &'static str {
    "/Library/Audio/Plug-Ins/CLAP/Surge XT.clap"
}

#[cfg(target_os = "linux")]
pub fn default_plugin_path() -> &'static str {
    "/usr/lib/clap/Surge XT.clap"
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_plugin_path() -> &'static str {
    ""
}

/// OS ごとのデフォルト Dexed パスを返す。
/// `active_plugin = 'Dexed'` の 1 行だけで使えるようにするための組み込み値。
/// 既知 OS でない場合は空文字を返す（ユーザーに設定を促す）。
#[cfg(target_os = "windows")]
pub fn default_dexed_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\Dexed.clap"
}

#[cfg(target_os = "macos")]
pub fn default_dexed_plugin_path() -> &'static str {
    "/Library/Audio/Plug-Ins/CLAP/Dexed.clap"
}

#[cfg(target_os = "linux")]
pub fn default_dexed_plugin_path() -> &'static str {
    "/usr/lib/clap/Dexed.clap"
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_dexed_plugin_path() -> &'static str {
    ""
}

/// OS ごとのデフォルト Vaporizer2 パスを返す。
/// `active_plugin = 'Vaporizer2'` の 1 行だけで使えるようにするための組み込み値。
/// 既知 OS でない場合は空文字を返す（ユーザーに設定を促す）。
///
/// **音色置き場の既定値は用意しない。** Vaporizer2 のプリセット置き場は
/// インストーラが決める固定の場所ではなく、ユーザーが
/// `%APPDATA%\Vaporizer2\VASTvaporizerSettings.xml` の `PresetRootFolder`
/// （またはレジストリ）で自由に決める。そこを読みに行くと個人のディレクトリ構成に
/// 依存するので、`patches_dirs` は config.toml に書いてもらう。
/// 書かれていないプロファイルは音色置き場が空になり、カタログに載らない
/// （`docs/adr/0005-...` の実在チェックと同じく、安全側に倒れる）。
#[cfg(target_os = "windows")]
pub fn default_vaporizer2_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\VASTvaporizer2.clap"
}

#[cfg(target_os = "macos")]
pub fn default_vaporizer2_plugin_path() -> &'static str {
    "/Library/Audio/Plug-Ins/CLAP/VASTvaporizer2.clap"
}

#[cfg(target_os = "linux")]
pub fn default_vaporizer2_plugin_path() -> &'static str {
    "/usr/lib/clap/VASTvaporizer2.clap"
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_vaporizer2_plugin_path() -> &'static str {
    ""
}

/// OS ごとのデフォルト patches_dirs を返す。
/// 既知 OS でない場合や取得できない場合は空配列を返す（ユーザーに設定を促す）。
#[cfg(target_os = "windows")]
pub fn default_patches_dirs() -> Vec<String> {
    vec![
        r"C:\ProgramData\Surge XT\patches_factory".to_string(),
        r"C:\ProgramData\Surge XT\patches_3rdparty".to_string(),
    ]
}

#[cfg(target_os = "macos")]
pub fn default_patches_dirs() -> Vec<String> {
    vec![
        "/Library/Application Support/Surge XT/patches_factory".to_string(),
        "/Library/Application Support/Surge XT/patches_3rdparty".to_string(),
    ]
}

#[cfg(target_os = "linux")]
pub fn default_patches_dirs() -> Vec<String> {
    dirs::data_dir()
        .map(|d| {
            vec![
                d.join("surge-data")
                    .join("patches_factory")
                    .to_string_lossy()
                    .into_owned(),
                d.join("surge-data")
                    .join("patches_3rdparty")
                    .to_string_lossy()
                    .into_owned(),
            ]
        })
        .unwrap_or_default()
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_patches_dirs() -> Vec<String> {
    Vec::new()
}

/// OS ごとのデフォルト Dexed cartridge ディレクトリを返す。
///
/// Dexed が初回起動時に factory cartridge を展開する場所。`.syx` 1 個が
/// 32 program に展開される（`cmrt_core::dx7`）。
/// 既知 OS でない場合や取得できない場合は空配列を返す（ユーザーに設定を促す）。
#[cfg(target_os = "windows")]
pub fn default_dexed_cartridge_dirs() -> Vec<String> {
    dirs::config_dir()
        .map(|dir| {
            vec![dir
                .join("DigitalSuburban")
                .join("Dexed")
                .join("Cartridges")
                .to_string_lossy()
                .into_owned()]
        })
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
pub fn default_dexed_cartridge_dirs() -> Vec<String> {
    dirs::data_dir()
        .map(|dir| {
            vec![dir
                .join("DigitalSuburban")
                .join("Dexed")
                .join("Cartridges")
                .to_string_lossy()
                .into_owned()]
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
pub fn default_dexed_cartridge_dirs() -> Vec<String> {
    dirs::data_dir()
        .map(|dir| {
            vec![dir
                .join("DigitalSuburban")
                .join("Dexed")
                .join("Cartridges")
                .to_string_lossy()
                .into_owned()]
        })
        .unwrap_or_default()
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_dexed_cartridge_dirs() -> Vec<String> {
    Vec::new()
}
