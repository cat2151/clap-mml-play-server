use super::*;

#[test]
fn the_plugin_id_or_the_file_name_finds_the_product() {
    assert_eq!(
        u_he_product_of(Some(ZEBRALETTE3_PLUGIN_ID), "whatever.clap").map(|p| p.product),
        Some("Zebralette3")
    );
    assert_eq!(
        u_he_product_of(None, r"C:\CLAP\u-he\Zebralette3.clap").map(|p| p.product),
        Some("Zebralette3")
    );
    assert_eq!(
        u_he_product_of(None, r"C:\CLAP\u-he\TyrellN6.clap").map(|p| p.product),
        Some("TyrellN6")
    );
    assert_eq!(u_he_product_of(Some("com.u-he.Diva"), "Diva.clap"), None);
}

/// TyrellN6 は `.h2p` の既定所有者なので空タグ、Zebralette3 は `z`。他の plugin は空。
#[test]
fn only_zebralette3_carries_a_patch_tag() {
    assert_eq!(patch_tag_of(Some(TYRELLN6_PLUGIN_ID), "x.clap"), "");
    assert_eq!(patch_tag_of(Some(ZEBRALETTE3_PLUGIN_ID), "x.clap"), "z");
    assert_eq!(patch_tag_of(None, r"C:\CLAP\u-he\Zebralette3.clap"), "z");
    assert_eq!(
        patch_tag_of(Some(crate::SURGE_XT_PLUGIN_ID), "Surge XT.clap"),
        ""
    );
}
