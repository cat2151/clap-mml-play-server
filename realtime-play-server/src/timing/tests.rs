use std::time::Duration;

use super::startup_phase_line;

#[test]
fn startup_phase_uses_the_same_phase_field_as_completion_timings() {
    assert_eq!(
        startup_phase_line("plugin_catalog", Duration::from_millis(37)),
        "cmrt-server-startup: phase=plugin_catalog event=begin since_boot_ms=37"
    );
}
