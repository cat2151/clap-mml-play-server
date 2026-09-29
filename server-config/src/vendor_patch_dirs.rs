//! 音色置き場をプラグイン本体の設定から読む（Vaporizer2・TyrellN6 は registry、Floe は floe.ini）。
//!
//! config.toml の `patches_dirs` は使わない。再インストールや本体側の設定変更のたびに
//! toml を書き直す二度手間になり、書き忘れると本体と食い違うため。

/// `InstallPath` の下の `Presets`。
pub(crate) fn vaporizer2_preset_dirs() -> Vec<String> {
    vaporizer2_install_path()
        .map(|install| vec![vaporizer2_presets_of(&install)])
        .unwrap_or_default()
}

fn vaporizer2_presets_of(install_path: &str) -> String {
    std::path::Path::new(install_path)
        .join("Presets")
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
fn vaporizer2_install_path() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\VAST Dynamics\Vaporizer2\Settings")
        .and_then(|key| key.get_value::<String, _>("InstallPath"))
        .ok()
        .filter(|path| !path.trim().is_empty())
}

#[cfg(not(windows))]
fn vaporizer2_install_path() -> Option<String> {
    None
}

/// `DataPath` の下の `Presets\TyrellN6`。`UserPresets` は含めない。
pub(crate) fn tyrelln6_preset_dirs() -> Vec<String> {
    tyrelln6_data_path()
        .map(|data| vec![tyrelln6_presets_of(&data)])
        .unwrap_or_default()
}

fn tyrelln6_presets_of(data_path: &str) -> String {
    std::path::Path::new(data_path)
        .join("Presets")
        .join("TyrellN6")
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
fn tyrelln6_data_path() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\u-he\TyrellN6")
        .and_then(|key| key.get_value::<String, _>("DataPath"))
        .ok()
        .filter(|path| !path.trim().is_empty())
}

#[cfg(not(windows))]
fn tyrelln6_data_path() -> Option<String> {
    None
}

/// Floe が scan する preset 置き場。
///
/// Floe 自身は `%PUBLIC%\Floe\Presets` と `extra-presets-folder` の両方を scan するが、
/// ここでは extra があれば extra だけを返す。両方を返すと置き場が別ドライブに分かれ、
/// 共通の親（display 文字列の基点）が無くなって保存済みの patch 文字列が指し先を失う。
pub(crate) fn floe_preset_dirs() -> Vec<String> {
    let Some(public) = floe_global_data_dir() else {
        return Vec::new();
    };
    let ini = public.join("Floe").join("Preferences").join("floe.ini");
    let extra = std::fs::read_to_string(ini)
        .map(|text| floe_extra_presets_folders(&text))
        .unwrap_or_default();
    if extra.is_empty() {
        vec![public
            .join("Floe")
            .join("Presets")
            .to_string_lossy()
            .into_owned()]
    } else {
        extra
    }
}

/// floe.ini の `extra-presets-folder = <path>` 行（複数可）。
fn floe_extra_presets_folders(ini: &str) -> Vec<String> {
    ini.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| key.trim() == "extra-presets-folder")
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

/// Floe の `KnownDirectoryType::GlobalData`（Windows は FOLDERID_Public）。
#[cfg(windows)]
fn floe_global_data_dir() -> Option<std::path::PathBuf> {
    Some(
        std::env::var_os("PUBLIC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Users\Public")),
    )
}

#[cfg(not(windows))]
fn floe_global_data_dir() -> Option<std::path::PathBuf> {
    None
}

#[cfg(test)]
mod tests;
