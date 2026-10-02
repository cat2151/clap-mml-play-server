use super::*;

const INIT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<VoyageVoyage INPUT="0.5" DECAY="0.5" ENGINE="3" TRUEST="0.0" BYPASS="0.0"/>"#;

fn saved_attributes(blob: &[u8]) -> Element {
    parse_voyage_xml(&juce_xml::decode(blob).unwrap()).unwrap()
}

fn attribute<'a>(element: &'a Element, name: &str) -> &'a str {
    element.attributes.get(name).map(String::as_str).unwrap()
}

#[test]
fn pst_attributes_overwrite_the_template_and_missing_ones_stay() {
    let init = juce_xml::encode(INIT_XML);
    let pst = r#"<?xml version="1.0" encoding="UTF-8"?>

<VoyageVoyage INPUT="0.25"
              DECAY="0.9" ENGINE="0" BYPASS="0.0"/>
"#;
    let state = saved_attributes(&voyage_state_blob(&init, pst).unwrap());
    assert_eq!(attribute(&state, "INPUT"), "0.25");
    assert_eq!(attribute(&state, "DECAY"), "0.9");
    assert_eq!(attribute(&state, "ENGINE"), "0");
    assert_eq!(attribute(&state, "TRUEST"), "0.0");
    assert_eq!(attribute(&state, "BYPASS"), "0.0");
    assert_eq!(state.attributes.len(), 5);
}

#[test]
fn pst_attribute_missing_from_the_template_is_an_error() {
    let init = juce_xml::encode(INIT_XML);
    let pst = r#"<VoyageVoyage DECAY="0.9" NEWPARAM="1.0"/>"#;
    let error = voyage_state_blob(&init, pst).unwrap_err();
    assert!(format!("{error:#}").contains("NEWPARAM"), "{error:#}");
}

#[test]
fn wrong_root_element_is_an_error() {
    let init = juce_xml::encode(INIT_XML);
    assert!(voyage_state_blob(&init, r#"<Other DECAY="0.9"/>"#).is_err());
    let other_state = juce_xml::encode(r#"<Other DECAY="0.5"/>"#);
    assert!(voyage_state_blob(&other_state, r#"<VoyageVoyage DECAY="0.9"/>"#).is_err());
}

#[test]
fn value_is_relative_path_without_extension_and_every_folder_is_shimmer_reverb() {
    let root = std::env::temp_dir().join("cmrt_test_voyage_voyage_value");
    let _ = std::fs::remove_dir_all(&root);
    for folder in ["Delay", "Spacecraft"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::write(root.join(folder).join("x.pst"), INIT_XML).unwrap();
    }
    for folder in ["Delay", "Spacecraft"] {
        let path = root.join(folder).join("x.pst");
        let value = voyage_voyage_value(&root, &path, "Voyage Voyage").unwrap();
        assert_eq!(value.value, format!("{folder}/x"));
        assert_eq!(value.shown, format!("{folder}/x"));
        assert_eq!(value.category, "Space / Imaging");
        assert_eq!(value.kind, "Shimmer Reverb");
    }
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn unreadable_pst_is_not_listed() {
    let root = std::env::temp_dir().join("cmrt_test_voyage_voyage_broken");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("broken.pst");
    std::fs::write(&path, "<Other/>").unwrap();
    assert!(voyage_voyage_value(&root, &path, "Voyage Voyage").is_err());
    std::fs::remove_dir_all(&root).unwrap();
}
