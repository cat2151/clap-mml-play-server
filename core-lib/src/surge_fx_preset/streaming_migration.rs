//! 古い `streaming_version` の `.srgfx` を、現行の parameter 意味へ合わせる。
//!
//! Surge の preset loader は読み込み後に各 effect の `handleStreamingMismatches()` を
//! 呼ぶ。plugin state 経路にはこれが無いので、同じ書き換えをここで行う。
//! 内容は Surge XT 1.3.4 の各 effect から写した（対象は `streaming_version` が
//! [`SURGE_STREAMING_REVISION`] より古い snapshot だけ）。
//!
//! Delay の feedback `deform_type = 1` は plugin の既定値と同じなので書かない。
//! Vocoder の revision 10 以前の移行は周波数表が要るので扱わない（factory preset の
//! 最小 revision は 15）。

use super::SurgeFxSnapshot;

/// Surge XT 1.3.4 の `ff_revision`。
pub const SURGE_STREAMING_REVISION: i32 = 24;

/// Ensemble の output filter 既定値（6 kHz を MIDI note 69 基準の semitone で表した値）。
const ENSEMBLE_DEFAULT_FILTER_CUT: f64 = 45.232_644_8;

/// `snapshot.streaming_version` が現行より古いときだけ、Surge と同じ移行を施す。
pub fn apply_streaming_migrations(snapshot: &mut SurgeFxSnapshot) {
    let revision = snapshot.streaming_version;
    if revision >= SURGE_STREAMING_REVISION {
        return;
    }
    let p = &mut snapshot.params;
    match snapshot.fx_type {
        // Delay
        1 => {
            if revision <= 15 {
                p[4].deactivated = false;
                p[5].deactivated = false;
                p[1].deactivated = false;
            }
            if revision <= 18 {
                p[2].extend_range = false;
            }
            if revision <= 21 {
                p[3].extend_range = false;
                p[7].extend_range = false;
            }
        }
        // Reverb 1
        2 => {
            if revision <= 15 {
                p[5].deactivated = false;
                p[8].deactivated = false;
            }
        }
        // Rotary Speaker
        4 => {
            if revision <= 12 {
                p[3].value = Some(0.7);
                p[4].value = Some(0.0);
                p[4].deactivated = true;
                p[5].value = Some(0.0);
                p[6].value = Some(1.0);
                p[7].value = Some(1.0);
            }
        }
        // Distortion
        5 => {
            if revision <= 11 {
                p[11].value = Some(0.0);
                p[0].extend_range = false;
                p[6].extend_range = false;
            }
            if revision <= 15 {
                p[3].deactivated = false;
                p[9].deactivated = false;
            }
        }
        // EQ
        6 => {
            if revision <= 12 {
                p[10].value = Some(1.0);
            }
            if revision <= 15 {
                p[0].deactivated = false;
                p[3].deactivated = false;
                p[6].deactivated = false;
            }
        }
        // Conditioner
        8 => {
            if revision <= 15 {
                p[0].deactivated = false;
                p[1].deactivated = false;
            }
            if revision <= 16 {
                p[8].value = Some(-60.0);
                p[8].deactivated = true;
            }
        }
        // Ring Modulator
        13 => {
            if revision <= 15 {
                p[6].deactivated = false;
                p[7].deactivated = false;
            }
        }
        // Ensemble
        20 => {
            if revision <= 22 {
                p[11].value = Some(ENSEMBLE_DEFAULT_FILTER_CUT);
                p[11].deactivated = true;
            }
        }
        // Combulator
        21 => {
            if revision <= 17 {
                p[5].deactivated = false;
            }
            if revision <= 20 {
                p[2].extend_range = false;
                p[3].extend_range = false;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
