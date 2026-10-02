//! Floe の音色置き場を `floe.ini` から読む。
//!
//! config.toml の `patches_dirs` は使わない。再インストールや本体側の設定変更のたびに
//! toml を書き直す二度手間になり、書き忘れると本体と食い違うため。

/// Floe が scan する preset 置き場。
///
/// Floe 自身は `%PUBLIC%\Floe\Presets` と `extra-presets-folder` の両方を scan するが、
/// ここでは extra があれば extra だけを返す。両方を返すと置き場が別ドライブに分かれ、
/// 共通の親（display 文字列の基点）が無くなって保存済みの patch 文字列が指し先を失う。
pub fn floe_preset_dirs() -> Vec<String> {
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
