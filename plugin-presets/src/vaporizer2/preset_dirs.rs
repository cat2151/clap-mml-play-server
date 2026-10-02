//! Vaporizer2 の音色置き場を registry の `InstallPath` から読む。
//!
//! config.toml の `patches_dirs` は使わない。再インストールや本体側の設定変更のたびに
//! toml を書き直す二度手間になり、書き忘れると本体と食い違うため。

/// `InstallPath` の下の `Presets`。
pub fn vaporizer2_preset_dirs() -> Vec<String> {
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

#[cfg(test)]
mod tests;
