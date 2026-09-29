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
/// `[plugins.Dexed]` を省略しても混在カタログへ載せられるようにするための組み込み値。
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
/// `[plugins.Vaporizer2]` で本体パスを省略できるようにするための組み込み値。
/// 既知 OS でない場合は空文字を返す（ユーザーに設定を促す）。
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

/// OS ごとのデフォルト Floe パスを返す。
/// `[plugins.Floe]` で本体パスを省略できるようにするための組み込み値。
#[cfg(target_os = "windows")]
pub fn default_floe_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\Floe.clap"
}

#[cfg(target_os = "macos")]
pub fn default_floe_plugin_path() -> &'static str {
    "/Library/Audio/Plug-Ins/CLAP/Floe.clap"
}

#[cfg(target_os = "linux")]
pub fn default_floe_plugin_path() -> &'static str {
    "/usr/lib/clap/Floe.clap"
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_floe_plugin_path() -> &'static str {
    ""
}

/// OS ごとのデフォルト sforzando パスを返す。
///
/// 音色置き場は ARIA の registry（user bank と installed bank）だけで決まり、
/// `[plugins.Sforzando].patches_dirs` は使わない。
#[cfg(target_os = "windows")]
pub fn default_sforzando_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\Plogue\sforzando_x64.clap"
}

#[cfg(target_os = "macos")]
pub fn default_sforzando_plugin_path() -> &'static str {
    "/Library/Audio/Plug-Ins/CLAP/sforzando.clap"
}

#[cfg(target_os = "linux")]
pub fn default_sforzando_plugin_path() -> &'static str {
    "/usr/lib/clap/sforzando.clap"
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_sforzando_plugin_path() -> &'static str {
    ""
}

/// OS ごとのデフォルト Six Sines パスを返す。
/// 音色置き場は GitHub から取得した factory 置き場（[`crate::six_sines_factory_dir`]）で決まる。
#[cfg(target_os = "windows")]
pub fn default_six_sines_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\BaconPaul\Six Sines.clap"
}

#[cfg(target_os = "macos")]
pub fn default_six_sines_plugin_path() -> &'static str {
    "/Library/Audio/Plug-Ins/CLAP/BaconPaul/Six Sines.clap"
}

#[cfg(target_os = "linux")]
pub fn default_six_sines_plugin_path() -> &'static str {
    "/usr/lib/clap/Six Sines.clap"
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_six_sines_plugin_path() -> &'static str {
    ""
}

/// OS ごとのデフォルト TyrellN6 パスを返す。Windows 以外は空。
/// 音色置き場は registry の `DataPath` から決まる（[`crate::merged_plugin_profiles`]）。
#[cfg(target_os = "windows")]
pub fn default_tyrelln6_plugin_path() -> &'static str {
    r"C:\Program Files\Common Files\CLAP\u-he\TyrellN6.clap"
}

#[cfg(not(target_os = "windows"))]
pub fn default_tyrelln6_plugin_path() -> &'static str {
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
