//! TyrellN6 の音色置き場を registry の `DataPath` から読む。
//!
//! config.toml の `patches_dirs` は使わない。再インストールや本体側の設定変更のたびに
//! toml を書き直す二度手間になり、書き忘れると本体と食い違うため。

/// `DataPath` の下の `Presets\TyrellN6`。`UserPresets` は含めない。
pub fn tyrelln6_preset_dirs() -> Vec<String> {
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

#[cfg(test)]
mod tests;
