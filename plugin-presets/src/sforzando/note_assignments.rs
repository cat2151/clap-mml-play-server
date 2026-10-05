//! kit 判定で使った狭い region に限定せず、全 note-on region の鍵割当を集約する。

use std::{collections::BTreeSet, path::Path};

use super::{drum_kit::raw_key_range, sfz_regions::complete_sfz_regions};

/// MIDI note number の昇順・重複なしの一覧。読み込みや鍵域の解釈失敗は全体を Err にする。
pub fn sfz_note_assignments(path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut notes = BTreeSet::new();
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
        notes.extend((lo.max(0)..=hi.min(127)).map(|note| note as u8));
    }
    Ok(notes.into_iter().collect())
}

#[cfg(test)]
mod tests;
