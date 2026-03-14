use super::timing::{compute_measure_samples, parse_beat_numerator, parse_tempo_bpm};

// ─── parse_tempo_bpm ──────────────────────────────────────────

#[test]
fn parse_tempo_bpm_basic() {
    assert_eq!(parse_tempo_bpm("t120cde"), Some(120.0));
}

#[test]
fn parse_tempo_bpm_at_start() {
    assert_eq!(parse_tempo_bpm("t80"), Some(80.0));
}

#[test]
fn parse_tempo_bpm_no_tempo() {
    assert_eq!(parse_tempo_bpm("cde"), None);
}

#[test]
fn parse_tempo_bpm_empty() {
    assert_eq!(parse_tempo_bpm(""), None);
}

#[test]
fn parse_tempo_bpm_after_json() {
    // JSON除去後の残りMMLにt200が含まれる場合
    assert_eq!(parse_tempo_bpm("t200efg"), Some(200.0));
}

// ─── parse_beat_numerator ─────────────────────────────────────

#[test]
fn parse_beat_numerator_4_4() {
    let json = r#"{"beat": "4/4"}"#;
    assert_eq!(parse_beat_numerator(Some(json)), 4);
}

#[test]
fn parse_beat_numerator_3_4() {
    let json = r#"{"beat": "3/4"}"#;
    assert_eq!(parse_beat_numerator(Some(json)), 3);
}

#[test]
fn parse_beat_numerator_no_json() {
    // JSONなし → デフォルト 4
    assert_eq!(parse_beat_numerator(None), 4);
}

#[test]
fn parse_beat_numerator_malformed_json() {
    // 壊れたJSON → デフォルト 4
    assert_eq!(parse_beat_numerator(Some("{invalid")), 4);
}

#[test]
fn parse_beat_numerator_missing_beat_key() {
    // beat キーなし → デフォルト 4
    let json = r#"{"other": "value"}"#;
    assert_eq!(parse_beat_numerator(Some(json)), 4);
}

#[test]
fn parse_beat_numerator_zero_clamps_to_one() {
    // beat 分子が 0 → 1 にクランプされる
    let json = r#"{"beat": "0/4"}"#;
    assert_eq!(parse_beat_numerator(Some(json)), 1);
}

#[test]
fn parse_beat_numerator_non_numeric() {
    // 数値でない beat → デフォルト 4
    let json = r#"{"beat": "x/4"}"#;
    assert_eq!(parse_beat_numerator(Some(json)), 4);
}

// ─── compute_measure_samples ──────────────────────────────────

#[test]
fn compute_measure_samples_4_4_t120_44100() {
    // t120, 4/4, 44100Hz: 4 * (60/120) * 44100 * 2 = 4 * 0.5 * 44100 * 2 = 176400
    let result = compute_measure_samples(4, 120.0, 44100.0);
    assert_eq!(result, 176400);
    assert_eq!(result % 2, 0, "ステレオ整列のため偶数であること");
}

#[test]
fn compute_measure_samples_result_is_always_even() {
    // どんな入力でも結果は偶数（ステレオ整列）
    for beat in [1u32, 2, 3, 4, 6] {
        for bpm in [60.0f64, 120.0, 96.0, 180.0] {
            let result = compute_measure_samples(beat, bpm, 44100.0);
            assert_eq!(result % 2, 0, "beat={beat}, bpm={bpm} のとき偶数でない: {result}");
        }
    }
}

#[test]
fn compute_measure_samples_clamps_bpm_zero() {
    // BPM 0 は 1.0 にクランプされ、OOM/パニックを起こさない
    let result = compute_measure_samples(4, 0.0, 44100.0);
    // 4 * 60 / 1.0 * 44100 * 2 = 21168000 (大きいが有限)
    assert!(result > 0);
    assert_eq!(result % 2, 0);
}

#[test]
fn compute_measure_samples_clamps_bpm_negative() {
    // 負のBPM は 1.0 にクランプ
    let result = compute_measure_samples(4, -100.0, 44100.0);
    assert!(result > 0);
    assert_eq!(result % 2, 0);
}

#[test]
fn compute_measure_samples_beat_zero_clamps_to_one() {
    // beat=0 は 1 にクランプ
    let result = compute_measure_samples(0, 120.0, 44100.0);
    // 1 * 60 / 120 * 44100 * 2 = 44100
    assert_eq!(result, 44100);
    assert_eq!(result % 2, 0);
}

// ─── ensure_cmrt_dir ──────────────────────────────────────────

#[test]
fn ensure_cmrt_dir_is_idempotent() {
    // 複数回呼んでもエラーにならない
    let r1 = crate::pipeline::ensure_cmrt_dir();
    let r2 = crate::pipeline::ensure_cmrt_dir();
    assert!(r1.is_ok(), "初回 ensure_cmrt_dir が失敗: {:?}", r1.err());
    assert!(r2.is_ok(), "2回目 ensure_cmrt_dir が失敗: {:?}", r2.err());
}
