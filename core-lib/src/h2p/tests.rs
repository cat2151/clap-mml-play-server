use super::*;

#[test]
fn recognizes_h2p_patches_case_insensitively() {
    assert!(is_h2p_patch_path("01 Basses/Abgrund.h2p"));
    assert!(is_h2p_patch_path("01 Basses\\Abgrund.H2P"));
    assert!(!is_h2p_patch_path(".h2p"));
    assert!(!is_h2p_patch_path("01 Basses/Abgrund.fxp"));
    assert!(!is_h2p_patch_path("Bass/Bass 1.sxsnp"));
}

#[test]
fn the_products_that_read_h2p() {
    assert_eq!(h2p_product_of(TYRELLN6_PLUGIN_ID), Some("TyrellN6"));
    assert_eq!(h2p_product_of(ZEBRALETTE3_PLUGIN_ID), Some("Zebralette3"));
    assert_eq!(h2p_product_of(crate::SURGE_XT_PLUGIN_ID), None);
}

/// plugin は他製品の `.h2p` も受け付けてしまうので、ここで両向きとも拒む。
#[test]
fn the_model_marker_must_name_the_product() {
    let z3 = b"/*@Meta\n*/\n\n#AM=Zebralette3\r\n#Vers=1\n";
    let n6 = b"#AM=TyrellN6\n#Vers=10010\n";
    assert!(check_h2p_model("Zebralette3", z3).is_ok());
    assert!(check_h2p_model("TyrellN6", n6).is_ok());

    let error = check_h2p_model("Zebralette3", n6).unwrap_err();
    assert!(
        error.contains("TyrellN6") && error.contains("Zebralette3"),
        "{error}"
    );
    let error = check_h2p_model("TyrellN6", z3).unwrap_err();
    assert!(
        error.contains("TyrellN6") && error.contains("Zebralette3"),
        "{error}"
    );
    assert!(check_h2p_model("TyrellN6", b"#Vers=1\n").is_err());
}
