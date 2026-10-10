use super::*;
use crate::patch_state_prepare::tests::PatchFile;

#[test]
fn pure_formats_validate_identity_and_data() {
    let bundle = Path::new("X:/plugins/test.clap");
    let cases = [
        (
            SIX_SINES_PLUGIN_ID,
            "sxsnp",
            b"<patch id=\"org.baconpaul.six-sines\"><params/></patch>".as_slice(),
        ),
        (
            TYRELLN6_PLUGIN_ID,
            "h2p",
            b"#AM=TyrellN6\n#Vers=10010\n".as_slice(),
        ),
        (
            VAPORIZER2_PLUGIN_ID,
            "vvp",
            b"<VASTvaporizer2><PARAM id=\"m_uPolyMode\" text=\"Poly16\"/></VASTvaporizer2>"
                .as_slice(),
        ),
    ];
    for (id, ext, bytes) in cases {
        let file = PatchFile::new(ext, bytes);
        assert!(!prepare_catalog_clap_patch_state(id, bundle, &file.0)
            .unwrap()
            .is_empty());
        std::fs::write(&file.0, b"broken").unwrap();
        assert!(matches!(
            prepare_catalog_clap_patch_state(id, bundle, &file.0),
            Err(PatchStateError::InvalidData { .. })
        ));
    }
    let other = PatchFile::new("sxsnp", b"<patch id=\"other\"><params/></patch>");
    assert!(prepare_catalog_clap_patch_state(SIX_SINES_PLUGIN_ID, bundle, &other.0).is_err());
    let other = PatchFile::new("h2p", b"#AM=Other\n");
    assert!(prepare_catalog_clap_patch_state(TYRELLN6_PLUGIN_ID, bundle, &other.0).is_err());
}
