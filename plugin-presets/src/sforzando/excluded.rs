//! Notice for SFZ files that sit under a root no ARIA program source covers.
//!
//! Files an installed bank leaves unregistered (`#include` parts, programs the bank omits) are
//! excluded silently: nobody on the user side can change that.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{canonical_key, path_is_within, strip_prefix_portable};

const EXAMPLE_LIMIT: usize = 2;

pub(super) struct ExcludedContext<'a> {
    pub(super) roots: &'a [PathBuf],
    /// Canonical keys of roots that have at least one `*.bank.xml` nearby.
    pub(super) bank_roots: &'a HashSet<String>,
    pub(super) user_root: Option<&'a Path>,
}

pub(super) fn notice(excluded: &[PathBuf], ctx: &ExcludedContext<'_>) -> Option<String> {
    let mut paths = excluded
        .iter()
        .filter_map(|path| {
            let root = ctx.roots.iter().find(|root| path_is_within(path, root))?;
            if ctx.bank_roots.contains(&canonical_key(root)) {
                return None;
            }
            let shown = strip_prefix_portable(path, root).unwrap_or_else(|| path.clone());
            Some(shown.display().to_string())
        })
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return None;
    }
    paths.sort();
    let destination = match ctx.user_root {
        Some(root) => format!("sforzando の user files directory ({})", root.display()),
        None => "sforzando で設定した user files directory".to_string(),
    };
    Some(format!(
        "ARIA の user files directory にも installed bank にも属さない SFZ {} 件を \
         catalog から除外 ({}); {destination} の下へ移すとロードできる",
        paths.len(),
        examples(&paths)
    ))
}

fn examples(paths: &[String]) -> String {
    let shown = paths
        .iter()
        .take(EXAMPLE_LIMIT)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let rest = paths.len().saturating_sub(EXAMPLE_LIMIT);
    if rest == 0 {
        shown
    } else {
        format!("{shown} 他 {rest} 件")
    }
}
