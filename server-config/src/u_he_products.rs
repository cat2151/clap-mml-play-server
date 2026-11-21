//! `.h2p` を読む u-he の plugin。
//!
//! 同じ `.h2p` を複数の製品が読むので、拡張子だけでは送り先が決まらない。製品ごとの patch タグで
//! 所有者を分け（`docs/adr/0026-patch-tag-decides-the-owner.md`）、ファイルの `#AM=` 行で中身が
//! その製品の音色かを確かめる。

use crate::{TYRELLN6_PLUGIN_ID, ZEBRALETTE3_PLUGIN_ID};

/// `.h2p` を読む u-he の製品 1 つ。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UHeProduct {
    pub plugin_id: &'static str,
    /// 製品名。registry の `Software\u-he\<product>`、音色置き場の `Presets\<product>`、
    /// 音色ファイルの `#AM=<product>` に現れる。
    pub product: &'static str,
    /// patch 文字列のタグ。既定所有者（TyrellN6）は空。
    pub patch_tag: &'static str,
    /// 音色が reverb・delay を内蔵しうるか。factory preset の `#Reverb=` / `#Delay=` 行で数えた。
    pub builtin_effects: bool,
}

/// Zebralette3 の音色を指す patch タグ（`x.h2p:z`）。
pub const ZEBRALETTE3_PATCH_TAG: &str = "z";

pub const U_HE_PRODUCTS: [UHeProduct; 2] = [
    UHeProduct {
        plugin_id: TYRELLN6_PLUGIN_ID,
        product: "TyrellN6",
        patch_tag: "",
        builtin_effects: false,
    },
    UHeProduct {
        plugin_id: ZEBRALETTE3_PLUGIN_ID,
        product: "Zebralette3",
        patch_tag: ZEBRALETTE3_PATCH_TAG,
        builtin_effects: true,
    },
];

/// その plugin が `.h2p` を読む u-he の製品か。`plugin_id` が無ければファイル名で見る。
pub fn u_he_product_of(plugin_id: Option<&str>, plugin_path: &str) -> Option<&'static UHeProduct> {
    U_HE_PRODUCTS.iter().find(|product| match plugin_id {
        Some(id) => id == product.plugin_id,
        None => crate::plugin_file_stem(plugin_path)
            .to_lowercase()
            .contains(&product.product.to_lowercase()),
    })
}

#[cfg(test)]
mod tests;
