use super::*;
use crate::surge_fx::juce_xml;

fn fxp(xml: &str) -> Vec<u8> {
    let chunk = juce_xml::encode(xml);
    let mut bytes = vec![0; 60];
    bytes[..4].copy_from_slice(b"CcnK");
    bytes[8..12].copy_from_slice(b"FPCh");
    bytes[16..20].copy_from_slice(b"OBXf");
    bytes[56..60].copy_from_slice(&(chunk.len() as u32).to_be_bytes());
    bytes.extend(chunk);
    bytes
}

#[test]
fn wraps_the_whole_program_and_preserves_streaming_version() {
    let xml = r#"<OB-Xf ob-xf_version="2025-12-13" programName="A &amp; B" Volume="0.25" author="Author"><VoiceMatrix><voice value="0.42"/></VoiceMatrix></OB-Xf>"#;
    // Both JUCE length conventions occur in installed factory content.
    for xml in [xml.to_string(), format!("{xml}\0")] {
        let bytes = obxf_state_blob(&fxp(&xml)).unwrap();
        let state_xml = juce_xml::decode(&bytes).unwrap();
        let state = xmltree::Element::parse(state_xml.as_bytes()).unwrap();
        assert_eq!(state.attributes["ob-xf_version"], "2025-12-13");
        assert_eq!(state.attributes["single-program-format"], "1");
        let mut expected = xmltree::Element::parse(xml.trim_end_matches('\0').as_bytes()).unwrap();
        expected.name = "program".into();
        assert_eq!(state.get_child("program"), Some(&expected));
    }
}

#[test]
fn rejects_other_plugins_truncated_chunks_and_invalid_programs() {
    let valid = fxp(r#"<OB-Xf ob-xf_version="2025-12-13"/>"#);
    for end in [0, 12, 59, valid.len() - 1] {
        assert!(obxf_state_blob(&valid[..end]).is_err());
    }
    let mut other = valid.clone();
    other[16..20].copy_from_slice(b"cjs3");
    assert!(obxf_state_blob(&other).is_err());
    let mut oversized = valid;
    oversized[56..60].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(obxf_state_blob(&oversized).is_err());
    for xml in [
        "broken",
        "<Other/>",
        "<OB-Xf/>",
        r#"<OB-Xf ob-xf_version="2025-12-13"><program/></OB-Xf>"#,
    ] {
        assert!(obxf_state_blob(&fxp(xml)).is_err(), "{xml}");
    }
}
