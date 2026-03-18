//! 拍子・テンポ解析とサンプル数計算の純粋関数

/// MML 文字列から最初の `tNNN` パターンを探し、BPM を返す
pub(super) fn parse_tempo_bpm(mml: &str) -> Option<f64> {
    let mut chars = mml.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == 't' {
            let mut num_str = String::new();
            while chars.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                num_str.push(chars.next().unwrap());
            }
            if !num_str.is_empty() {
                return num_str.parse().ok();
            }
        }
    }
    None
}

/// JSON 文字列から beat 分子を解析する。
/// `{"beat": "4/4"}` → 4。解析できない場合は 4 (4/4デフォルト) を返す。最小値は 1。
pub(super) fn parse_beat_numerator(json_str: Option<&str>) -> u32 {
    json_str
        .and_then(|j| {
            let v: serde_json::Value = serde_json::from_str(j).ok()?;
            let s = v.get("beat")?.as_str()?;
            s.split('/').next()?.parse::<u32>().ok()
        })
        .unwrap_or(4)
        .max(1)
}

/// BPM と beat 分子からステレオサンプル数を計算する純粋関数。
/// BPM は [1.0, 960.0] にクランプする。結果はステレオ整列のため偶数にする。
pub(super) fn compute_measure_samples(beat: u32, bpm: f64, sample_rate: f64) -> usize {
    let bpm = bpm.clamp(1.0, 960.0);
    let beat = beat.max(1);
    let secs = (beat as f64 * 60.0) / bpm;
    let raw = (secs * sample_rate * 2.0).round() as usize;
    // ステレオ (L/R インターリーブ) 整列のため偶数に切り上げる
    raw + (raw & 1)
}

#[cfg(test)]
mod tests {
    use super::{compute_measure_samples, parse_beat_numerator, parse_tempo_bpm};

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
}
