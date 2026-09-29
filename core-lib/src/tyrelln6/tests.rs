use super::*;

#[test]
fn recognizes_tyrelln6_patches_case_insensitively() {
    assert!(is_tyrelln6_patch_path("01 Basses/Abgrund.h2p"));
    assert!(is_tyrelln6_patch_path("01 Basses\\Abgrund.H2P"));
    assert!(!is_tyrelln6_patch_path(".h2p"));
    assert!(!is_tyrelln6_patch_path("01 Basses/Abgrund.fxp"));
    assert!(!is_tyrelln6_patch_path("Bass/Bass 1.sxsnp"));
}
