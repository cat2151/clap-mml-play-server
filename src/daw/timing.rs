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
