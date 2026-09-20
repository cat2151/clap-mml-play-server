use super::*;
use crate::surge_fx_preset::{SurgeFxParam, SURGE_FX_PARAM_COUNT};

fn snapshot(fx_type: i32, streaming_version: i32) -> SurgeFxSnapshot {
    SurgeFxSnapshot {
        name: "x".to_string(),
        fx_type,
        streaming_version,
        params: [SurgeFxParam::default(); SURGE_FX_PARAM_COUNT],
    }
}

#[test]
fn old_delay_presets_lose_their_deactivated_flags() {
    let mut old = snapshot(1, 15);
    old.params[4].deactivated = true;
    old.params[1].deactivated = true;
    apply_streaming_migrations(&mut old);
    assert!(!old.params[4].deactivated && !old.params[1].deactivated);

    let mut new = snapshot(1, 22);
    new.params[4].deactivated = true;
    apply_streaming_migrations(&mut new);
    assert!(new.params[4].deactivated);
}

#[test]
fn ensemble_before_output_filter_gets_the_filter_deactivated() {
    let mut old = snapshot(20, 22);
    apply_streaming_migrations(&mut old);
    assert!(old.params[11].deactivated);
    assert_eq!(old.params[11].value, Some(ENSEMBLE_DEFAULT_FILTER_CUT));
}

#[test]
fn current_revision_is_untouched() {
    let mut current = snapshot(8, SURGE_STREAMING_REVISION);
    current.params[0].deactivated = true;
    let before = current.clone();
    apply_streaming_migrations(&mut current);
    assert_eq!(current, before);
}
