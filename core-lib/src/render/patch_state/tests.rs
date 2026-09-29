use super::*;
use cmrt_server_config::SURGE_XT_PLUGIN_ID;

#[test]
fn generic_state_files_are_refused_by_own_format_only_plugins() {
    for id in [SIX_SINES_PLUGIN_ID, TYRELLN6_PLUGIN_ID] {
        let error = ensure_accepts_generic_state_file(id, "Pads/Pad 1.fxp").unwrap_err();
        assert!(error.to_string().contains(id), "{error:#}");
    }
    assert!(ensure_accepts_generic_state_file(SURGE_XT_PLUGIN_ID, "Pads/Pad 1.fxp").is_ok());
}
