use super::*;

#[test]
fn a_non_sforzando_instance_is_rejected_before_file_access() {
    let error = ensure_sforzando_capable("org.example.other", "missing.sfz").unwrap_err();

    let message = error.to_string();
    assert!(message.contains("missing.sfz"), "{message}");
    assert!(message.contains(SFORZANDO_PLUGIN_ID), "{message}");
    assert!(message.contains("org.example.other"), "{message}");
    assert!(!message.contains("canonicalize"), "{message}");
}
