//! `.sfz` が鍵ごとに別の音を割り当てた kit（drum kit・効果音 kit）かを、plugin を起動せずに判定する。
//!
//! 半音ごとに sample を録った旋律楽器も「1 鍵 1 region」になるので、鍵域の形だけでは区別できない。
//! kit は鍵ごとに sample 名の語幹（音名・数字・強弱記号を除いた残り）が違い、旋律楽器は同じ語幹が
//! 並ぶ。sample 名で区別を付けない kit のために、鳴る region がすべて 2 鍵以上で
//! `pitch_keytrack=0`（鍵を変えても音高が変わらない）であり、その鍵域が複数あることも kit の印とする。
//! 一部の region だけが該当するもの（旋律楽器に重ねたノイズ層）や、鍵域が 1 つだけのもの
//! （全鍵で同じ音）は kit ではない。

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use super::sfz_regions::{sfz_regions, SfzRegion};

/// 1〜2 鍵幅の region が覆う鍵が、これ未満なら kit と見なさない。
const MIN_NARROW_KEYS: usize = 2;
/// 1〜2 鍵幅の鍵のうち、語幹の組が鍵ごとに違う割合の下限（被覆率の条件と組で使う）。
const MIN_DISTINCT_RATIO: f64 = 0.4;
/// 鳴る鍵のうち 1〜2 鍵幅の region が覆う割合の下限。
const MIN_NARROW_COVERAGE: f64 = 0.4;
/// 被覆率を問わず kit と見なす、語幹の組が鍵ごとに違う割合。上の鍵域にピッチ付きの音を
/// 足した kit（中央がドラム、上が伸ばしたシンセ）を拾う。
const MIN_DISTINCT_RATIO_ALONE: f64 = 0.8;
/// [`MIN_DISTINCT_RATIO_ALONE`] で判定するときに要る 1〜2 鍵幅の鍵の数。
const MIN_NARROW_KEYS_ALONE: usize = 8;

/// 語幹から除く、強弱・奏法・round robin などの印。
const VARIATION_WORDS: [&str; 29] = [
    "rr", "vl", "vel", "v", "l", "r", "ff", "f", "mf", "mp", "p", "pp", "ppp", "fff", "sus",
    "stac", "hard", "soft", "med", "loud", "quiet", "dyn", "layer", "up", "down", "pb", "loop",
    "sum", "gext",
];

/// `path` の `.sfz` が kit か。ルートの `.sfz` を読めないときだけ `Err` を返す。
pub fn sfz_is_drum_kit(path: &Path) -> anyhow::Result<bool> {
    Ok(is_drum_kit(&sfz_regions(path)?))
}

fn is_drum_kit(regions: &[SfzRegion]) -> bool {
    let mut attack_regions = 0usize;
    let mut flat_wide_ranges = BTreeSet::new();
    let mut all_flat_wide = true;
    let mut sounding_keys = BTreeSet::new();
    let mut narrow_keys: HashMap<u8, BTreeSet<String>> = HashMap::new();
    for region in regions {
        let Some(sample) = region.get("sample") else {
            continue;
        };
        if !region
            .get("trigger")
            .is_none_or(|trigger| trigger.eq_ignore_ascii_case("attack"))
        {
            continue;
        }
        let Some((lo, hi)) = key_range(region) else {
            continue;
        };
        attack_regions += 1;
        sounding_keys.extend(lo..=hi);
        if hi > lo && keytrack_is_zero(region) {
            flat_wide_ranges.insert((lo, hi));
        } else {
            all_flat_wide = false;
        }
        if hi - lo <= 1 {
            let signature = sound_signature(sample, region);
            for key in lo..=hi {
                narrow_keys
                    .entry(key)
                    .or_default()
                    .insert(signature.clone());
            }
        }
    }
    if attack_regions > 0 && all_flat_wide && flat_wide_ranges.len() >= 2 {
        return true;
    }
    if narrow_keys.len() < MIN_NARROW_KEYS {
        return false;
    }
    let distinct = narrow_keys.values().collect::<BTreeSet<_>>().len();
    let distinct_ratio = distinct as f64 / narrow_keys.len() as f64;
    let coverage = narrow_keys.len() as f64 / sounding_keys.len() as f64;
    (distinct_ratio >= MIN_DISTINCT_RATIO && coverage >= MIN_NARROW_COVERAGE)
        || (distinct_ratio >= MIN_DISTINCT_RATIO_ALONE
            && narrow_keys.len() >= MIN_NARROW_KEYS_ALONE)
}

/// region が鳴る鍵の範囲。`key=` は `lokey=` / `hikey=` より優先する。読めない鍵名は `None`。
fn key_range(region: &SfzRegion) -> Option<(u8, u8)> {
    let (lo, hi) = match region.get("key") {
        Some(key) => {
            let key = midi_key(key)?;
            (key, key)
        }
        None => (
            region.get("lokey").map_or(Some(0), |key| midi_key(key))?,
            region.get("hikey").map_or(Some(127), |key| midi_key(key))?,
        ),
    };
    let (lo, hi) = (lo.clamp(0, 127), hi.clamp(0, 127));
    (lo <= hi).then_some((lo as u8, hi as u8))
}

/// MIDI 番号か音名（`c4` = 60、`c#4` / `db4` = 61）。
fn midi_key(text: &str) -> Option<i32> {
    let text = text.trim().to_ascii_lowercase();
    if let Ok(number) = text.parse::<i32>() {
        return Some(number);
    }
    let mut chars = text.chars();
    let base = match chars.next()? {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (accidental, octave) = match rest.chars().next() {
        Some('#') => (1, &rest[1..]),
        Some('b') => (-1, &rest[1..]),
        _ => (0, rest),
    };
    Some(base + accidental + (octave.parse::<i32>().ok()? + 1) * 12)
}

fn keytrack_is_zero(region: &SfzRegion) -> bool {
    region
        .get("pitch_keytrack")
        .and_then(|value| value.trim().parse::<f64>().ok())
        .is_some_and(|keytrack| keytrack == 0.0)
}

/// 鍵ごとの音の見分け。内蔵波形（`*sine` など）は名前が同じでも合成パラメータで別の音になる。
fn sound_signature(sample: &str, region: &SfzRegion) -> String {
    let sample = sample.trim();
    if !sample.starts_with('*') {
        return sample_stem(sample);
    }
    region
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "key" | "lokey" | "hikey" | "lovel" | "hivel"))
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("|")
}

/// sample のファイル名から音名・数字・強弱などの印を除いた語幹。
fn sample_stem(sample: &str) -> String {
    let file = sample.rsplit(['/', '\\']).next().unwrap_or(sample);
    let file = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let file = file.to_ascii_lowercase();
    file.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '#'))
        .filter(|token| !token.is_empty() && !is_note_token(token))
        .map(|token| token.replace(|ch: char| ch.is_ascii_digit() || ch == '#', ""))
        .filter(|token| !token.is_empty() && !VARIATION_WORDS.contains(&token.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 数字だけか、前後に数字を伴ってよい音名（`c`、`3`、`c4`、`4c#`、`ab`、`fs3`、`dsharp2`）。
fn is_note_token(token: &str) -> bool {
    let body = token.trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '-');
    let body = body.trim_end_matches(|ch: char| ch.is_ascii_digit() || ch == '-');
    if body.is_empty() {
        return true;
    }
    let Some(accidental) = body.strip_prefix(['a', 'b', 'c', 'd', 'e', 'f', 'g']) else {
        return false;
    };
    matches!(accidental, "" | "#" | "b" | "s" | "sharp")
}

#[cfg(test)]
mod tests;
