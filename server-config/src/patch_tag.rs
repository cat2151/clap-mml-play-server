//! 拡張子を共有する plugin を音色指定のタグで区別する。

/// その plugin の音色を指すタグ。拡張子の既定所有者なら空。
pub fn patch_tag_of(plugin_id: Option<&str>, plugin_path: &str) -> &'static str {
    if plugin_presets::obxf::is_obxf(plugin_id, plugin_path) {
        return crate::OBXF_PATCH_TAG;
    }
    crate::u_he_product_of(plugin_id, plugin_path).map_or("", |product| product.patch_tag)
}
