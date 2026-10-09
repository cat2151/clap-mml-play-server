//! kit の割当 note と、その鍵で鳴る音の表示名。
//!
//! 名前は音源定義に書かれた文字列だけから作る。楽器名を推測して補わない。

use std::collections::BTreeMap;

/// kit の 1 鍵。`name` は音源定義から名前を得られなかった鍵では `None`。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DrumKitNote {
    pub note: u8,
    pub name: Option<String>,
}

/// 名前の出どころ。値が小さいほど優先し、鍵ごとに最上位の出どころの名前だけを使う。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum NameSource {
    /// 鍵そのものに付いた表示ラベル（SFZ `label_keyN`、Floe named key range）。
    KeyLabel,
    /// SFZ `region_label`。
    RegionLabel,
    /// SFZ `group_label`。
    GroupLabel,
    /// sample ファイル名（拡張子を除く）。
    Sample,
}

/// 鍵ごとに名前候補を出現順に集める。
#[derive(Default)]
pub(crate) struct NoteNames {
    notes: BTreeMap<u8, Option<(NameSource, Vec<String>)>>,
}

impl NoteNames {
    /// 名前候補なしで鍵だけを登録する。
    pub fn insert_note(&mut self, note: u8) {
        self.notes.entry(note).or_default();
    }

    pub fn insert(&mut self, note: u8, source: NameSource, name: &str) {
        let name = name.trim().trim_matches('"').trim();
        if name.is_empty() {
            self.insert_note(note);
            return;
        }
        let entry = self.notes.entry(note).or_default();
        match entry {
            Some((current, _)) if *current < source => {}
            Some((current, names)) if *current == source => {
                if !names.iter().any(|known| known == name) {
                    names.push(name.to_string());
                }
            }
            _ => *entry = Some((source, vec![name.to_string()])),
        }
    }

    /// 昇順・重複なしの鍵一覧。同じ鍵に複数の名前があれば、最初の名前に残りの件数を添える。
    pub fn finish(self) -> Vec<DrumKitNote> {
        self.notes
            .into_iter()
            .map(|(note, names)| DrumKitNote {
                note,
                name: names.map(|(_, names)| match names.len() {
                    1 => names[0].clone(),
                    count => format!("{} +{}", names[0], count - 1),
                }),
            })
            .collect()
    }
}

/// sample 指定から、ディレクトリと拡張子を除いたファイル名。`*sine` などの合成波形はそのまま。
///
/// 末尾の未定義の `$変数`（拡張子の代わりに置かれる `$GEXT2` など）も除く。
pub(crate) fn sample_name(sample: &str) -> &str {
    let file = sample.trim().rsplit(['/', '\\']).next().unwrap_or_default();
    let stem = match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => file,
    };
    match stem.rsplit_once('$') {
        Some((name, variable))
            if !name.is_empty()
                && !variable.is_empty()
                && variable
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_') =>
        {
            name
        }
        _ => stem,
    }
}

#[cfg(test)]
mod tests;
