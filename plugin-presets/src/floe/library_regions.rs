//! Lua 評価後の region を、Floe と同じ自動 key mapping で確定する。

use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Library {
    pub id: u64,
    pub instruments: BTreeMap<String, Vec<Region>>,
    /// instrument ID ごとの `add_named_key_range`。鍵域は region と同じ instrument の鍵で表す。
    pub named_key_ranges: BTreeMap<String, Vec<NamedKeyRange>>,
}

pub(super) struct Region {
    /// sample ファイル名（拡張子を除く）。
    pub name: String,
    pub root: u8,
    pub low: u8,
    pub end: u8,
    pub note_on: bool,
    pub auto_map: Option<String>,
}

/// `low..end` の鍵に付いた表示名。
pub(super) struct NamedKeyRange {
    pub name: String,
    pub low: u8,
    pub end: u8,
}

impl Library {
    pub fn finish_mapping(&mut self) {
        for regions in self.instruments.values_mut() {
            let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for (i, region) in regions.iter().enumerate() {
                if let Some(group) = &region.auto_map {
                    groups.entry(group.clone()).or_default().push(i);
                }
            }
            // Floe maps all regions within each group, before selecting note-on triggers.
            // See sample_library_lua.cpp::ReadLua (auto_map_groups).
            for indices in groups.values_mut() {
                // Match Floe's prepend + linked-list quicksort, including equal-root ordering.
                // That order matters when note-on/off regions share an auto-map root.
                indices.reverse();
                floe_root_order(indices, regions);
                let mut previous_end = 0;
                for (position, &index) in indices.iter().enumerate() {
                    let next_root = indices.get(position + 1).map_or(128, |i| regions[*i].root);
                    let region = &mut regions[index];
                    region.low = previous_end;
                    region.end = if next_root == 128 {
                        128
                    } else {
                        region.root + (next_root - region.root) / 2 + 1
                    };
                    previous_end = region.end;
                }
            }
        }
    }
}

fn floe_root_order(indices: &mut [usize], regions: &[Region]) {
    if indices.len() < 2 {
        return;
    }
    let mut pending = vec![(0, indices.len() - 1)];
    while let Some((first, last)) = pending.pop() {
        if first == last {
            continue;
        }
        let mut pivot = first;
        let mut moving = first;
        for front in first..last {
            if regions[indices[front]].root < regions[indices[last]].root {
                pivot = moving;
                indices.swap(moving, front);
                moving += 1;
            }
        }
        indices.swap(moving, last);
        if first != pivot {
            pending.push((first, pivot));
        }
        if pivot < last {
            pending.push((pivot + 1, last));
        }
    }
}

pub(super) fn library_hash(id: &str) -> u64 {
    id.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}
