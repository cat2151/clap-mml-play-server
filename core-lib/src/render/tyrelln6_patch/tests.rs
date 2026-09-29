use super::*;
use cmrt_server_config::{SIX_SINES_PLUGIN_ID, SURGE_XT_PLUGIN_ID, VAPORIZER2_PLUGIN_ID};

#[test]
fn only_tyrelln6_accepts_h2p() {
    assert!(ensure_tyrelln6_capable(TYRELLN6_PLUGIN_ID).is_ok());
    for other in [
        SURGE_XT_PLUGIN_ID,
        VAPORIZER2_PLUGIN_ID,
        SIX_SINES_PLUGIN_ID,
    ] {
        let error = ensure_tyrelln6_capable(other).unwrap_err();
        assert!(error.to_string().contains(TYRELLN6_PLUGIN_ID), "{error:#}");
    }
}

/// 小さい block では frame 数で、大きい block ではブロック数で決まる。
#[test]
fn settle_covers_both_the_frame_and_the_block_minimum() {
    assert_eq!(state_settle_blocks(128), 32);
    assert_eq!(state_settle_blocks(512), 8);
    assert_eq!(state_settle_blocks(1000), 5);
    assert_eq!(state_settle_blocks(2048), 4);
    assert_eq!(state_settle_blocks(8192), 4);
}
