//! 単体で開いても鳴らない program の判定。user 側に対処がないので、外しても通知しない。
//!
//! - 他の program に `#include` される部品。親の `<control>` の `set_ccN` や `sw_default` が
//!   前提で、単体では CC が 0 のまま（`locc` 条件）や key switch 待ちになり、
//!   どの note を送っても鳴らないことがある。音はどれも親から出せる
//! - sample がどれも見つからない sfz

use std::collections::{BTreeMap, HashSet};

use super::{canonical_key, sample_weight, SforzandoProgramRef};

/// 鳴らない program の canonical key を、外す理由ごとに分けたもの。両方に入る key もある。
#[derive(Debug, Default)]
pub(super) struct UnplayableParts {
    /// どれかの program の走査で `#include` から辿った sfz（入れ子も含む）。
    included: HashSet<String>,
    all_samples_missing: HashSet<String>,
}

impl UnplayableParts {
    /// 走査できなかった program は、どちらにも入れない。
    pub fn find(programs: &BTreeMap<String, SforzandoProgramRef>) -> Self {
        let mut parts = Self::default();
        for (key, program) in programs {
            let Ok(scan) = sample_weight::scan_sfz(&program.sfz_path) else {
                continue;
            };
            if scan.weight.all_samples_missing() {
                parts.all_samples_missing.insert(key.clone());
            }
            parts.included.extend(
                scan.includes
                    .iter()
                    .filter_map(|path| crate::lexical_absolute(path).ok())
                    .map(|path| canonical_key(&path))
                    .filter(|include| include != key),
            );
        }
        parts
    }

    pub fn contains(&self, key: &str) -> bool {
        self.included.contains(key) || self.all_samples_missing.contains(key)
    }
}
