//! kit 判定で使った狭い region に限定せず、全 note-on region の鍵割当と表示名を集約する。
//!
//! 名前は `label_keyN`、`region_label`、`group_label`、sample ファイル名の順に優先する。
//! `region_label` / `group_label` は、同じ値を持つ region の鍵域がすべて同じときだけ使う。
//! 別の鍵域にまたがる値は round robin や group の容れ物の名前で、鳴る音を区別しないため。

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use super::{
    drum_kit::raw_key_range,
    sfz_regions::{complete_sfz_regions, SfzRegion},
};
use crate::drum_kit_note::{sample_name, DrumKitNote, NameSource, NoteNames};

const LABELS: [(&str, NameSource); 2] = [
    ("region_label", NameSource::RegionLabel),
    ("group_label", NameSource::GroupLabel),
];

/// MIDI note number の昇順・重複なしの一覧。読み込みや鍵域の解釈失敗は全体を Err にする。
pub fn sfz_note_assignments(path: &Path) -> anyhow::Result<Vec<DrumKitNote>> {
    let sounding = sounding_regions(path)?;
    let mut label_ranges: BTreeMap<(&str, &str), BTreeSet<(i32, i32)>> = BTreeMap::new();
    for (lo, hi, region) in &sounding {
        for (opcode, _) in LABELS {
            if let Some(label) = region.get(opcode) {
                label_ranges
                    .entry((opcode, label))
                    .or_default()
                    .insert((*lo, *hi));
            }
        }
    }
    let mut notes = NoteNames::default();
    for (lo, hi, region) in &sounding {
        for note in *lo..=*hi {
            let note = note as u8;
            if let Some(label) = region.get(&format!("label_key{note}")) {
                notes.insert(note, NameSource::KeyLabel, label);
            }
            for (opcode, source) in LABELS {
                if let Some(label) = region.get(opcode) {
                    if label_ranges[&(opcode, label.as_str())].len() == 1 {
                        notes.insert(note, source, label);
                    }
                }
            }
            notes.insert(note, NameSource::Sample, sample_name(&region["sample"]));
        }
    }
    Ok(notes.finish())
}

/// 鳴る region がすべて `loop_mode=one_shot` の鍵。note off で止まらないので、音長を持たなくてよい。
/// 昇順・重複なし。失敗の条件は [`sfz_note_assignments`] と同じ。
pub fn sfz_one_shot_notes(path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut one_shot: BTreeMap<u8, bool> = BTreeMap::new();
    for (lo, hi, region) in sounding_regions(path)? {
        let region_one_shot = region
            .get("loop_mode")
            .or_else(|| region.get("loopmode"))
            .is_some_and(|mode| mode.eq_ignore_ascii_case("one_shot"));
        for note in lo..=hi {
            *one_shot.entry(note as u8).or_insert(true) &= region_one_shot;
        }
    }
    Ok(one_shot
        .into_iter()
        .filter_map(|(note, one_shot)| one_shot.then_some(note))
        .collect())
}

/// note-on で鳴る region と、MIDI note に切り詰めた鍵域。
fn sounding_regions(path: &Path) -> anyhow::Result<Vec<(i32, i32, SfzRegion)>> {
    let mut sounding = Vec::new();
    for region in complete_sfz_regions(path)? {
        if !region.contains_key("sample") {
            continue;
        }
        match region.get("trigger").map(|s| s.to_ascii_lowercase()) {
            None => {}
            Some(trigger) if matches!(trigger.as_str(), "attack" | "first" | "legato") => {}
            Some(trigger) if matches!(trigger.as_str(), "release" | "release_key") => continue,
            Some(trigger) => anyhow::bail!("sfz の trigger を解釈できない: {trigger}"),
        }
        let (lo, hi) = raw_key_range(&region)
            .ok_or_else(|| anyhow::anyhow!("sfz の鍵域を解釈できない: {region:?}"))?;
        // Intersect with MIDI notes; key=-1/128 must not turn into an assigned endpoint.
        sounding.push((lo.max(0), hi.min(127), region));
    }
    Ok(sounding)
}

#[cfg(test)]
mod tests;
