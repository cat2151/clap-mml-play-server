//! u-he の音色ファイル（`.h2p`）。
//!
//! 中身は `#AM=<製品名>` を含む u-he のテキスト形式（途中に NUL を含む）で、各製品の
//! `clap.state` がそのまま読む形。TyrellN6 と Zebralette3 が同じ拡張子を読み、しかも plugin は
//! 他製品の `.h2p` も拒否せず state を書き換えるので、送る前に `#AM=` 行で製品を確かめる。

use cmrt_server_config::U_HE_PRODUCTS;
pub use cmrt_server_config::{TYRELLN6_PLUGIN_ID, ZEBRALETTE3_PLUGIN_ID};

const H2P_EXTENSION: &str = ".h2p";
const PATH_SEPARATORS: [char; 2] = ['/', '\\'];
const MODEL_MARKER: &[u8] = b"#AM=";

/// patch path が `.h2p` の音色を指しているか。拡張子の大小文字は区別しない。
pub fn is_h2p_patch_path(patch: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(|component| {
        component.len() > H2P_EXTENSION.len()
            && component
                .get(component.len() - H2P_EXTENSION.len()..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(H2P_EXTENSION))
    })
}

/// `.h2p` を読む u-he の製品名。その plugin が `.h2p` を読まなければ `None`。
pub fn h2p_product_of(plugin_id: &str) -> Option<&'static str> {
    U_HE_PRODUCTS
        .iter()
        .find(|product| product.plugin_id == plugin_id)
        .map(|product| product.product)
}

/// `.h2p` の中身が `product` の音色か（`#AM=<product>` 行があるか）。
pub fn check_h2p_model(product: &str, bytes: &[u8]) -> Result<(), String> {
    let mut models = bytes
        .split(|b| *b == b'\n')
        .filter_map(|line| line.strip_prefix(MODEL_MARKER))
        .map(|model| model.strip_suffix(b"\r").unwrap_or(model));
    match models.next() {
        Some(model) if model == product.as_bytes() => Ok(()),
        Some(model) => Err(format!(
            "{} の音色なので {product} では読めない",
            String::from_utf8_lossy(model)
        )),
        None => Err(format!("{product} の model marker（#AM=）が無い")),
    }
}

#[cfg(test)]
mod tests;
