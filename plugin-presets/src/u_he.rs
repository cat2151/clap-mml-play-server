//! u-he の plugin（TyrellN6・Zebralette3 など）の音色置き場を registry の `DataPath` から読む。
//!
//! config.toml の `patches_dirs` は使わない。再インストールや本体側の設定変更のたびに
//! toml を書き直す二度手間になり、書き忘れると本体と食い違うため。

/// `HKCU\Software\u-he\<product>` の `DataPath` の下の `Presets\<product>`。
/// `UserPresets` は含めない。`DataPath` が無い・空なら空の一覧。
pub fn u_he_preset_dirs(product: &str) -> Vec<String> {
    preset_dirs_of(data_path(product), product)
}

fn preset_dirs_of(data_path: Option<String>, product: &str) -> Vec<String> {
    data_path
        .filter(|path| !path.trim().is_empty())
        .map(|data| {
            vec![std::path::Path::new(&data)
                .join("Presets")
                .join(product)
                .to_string_lossy()
                .into_owned()]
        })
        .unwrap_or_default()
}

#[cfg(windows)]
fn data_path(product: &str) -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(format!(r"Software\u-he\{product}"))
        .and_then(|key| key.get_value::<String, _>("DataPath"))
        .ok()
}

#[cfg(not(windows))]
fn data_path(_product: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests;
