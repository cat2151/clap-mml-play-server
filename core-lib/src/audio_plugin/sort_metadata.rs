//! 音色一覧の並べ替え用に、表示名からカテゴリ・vendor を読み取る。

use super::PatchSortMetadata;
use crate::is_vvp_patch_path;

pub fn patch_sort_metadata(path: &str) -> PatchSortMetadata {
    let path = path.trim_matches(['/', '\\']);
    if is_vvp_patch_path(path) {
        return vvp_sort_metadata(path);
    }
    if has_surge_prefix(path) {
        return surge_sort_metadata(path);
    }
    first_segment_metadata(path)
}

pub(super) fn has_surge_prefix(path: &str) -> bool {
    path.starts_with("patches_factory/") || path.starts_with("patches_3rdparty/")
}

fn split_first(path: &str) -> (&str, &str) {
    path.split_once('/').unwrap_or((path, ""))
}

fn first_segment_metadata(path: &str) -> PatchSortMetadata {
    let (category, rest) = split_first(path);
    PatchSortMetadata {
        category: category.to_string(),
        source_rank: 0,
        vendor: String::new(),
        category_rest: rest.to_string(),
        path_rest: path.to_string(),
    }
}

fn surge_sort_metadata(path: &str) -> PatchSortMetadata {
    if let Some(rest) = path.strip_prefix("patches_factory/") {
        let (category, rest) = split_first(rest);
        return PatchSortMetadata {
            category: category.to_string(),
            source_rank: 0,
            vendor: String::new(),
            category_rest: rest.to_string(),
            path_rest: rest_path(path, "patches_factory/"),
        };
    }
    let rest = path.strip_prefix("patches_3rdparty/").unwrap_or(path);
    let (first, rest) = split_first(rest);
    let (category, vendor, category_rest) = if rest.is_empty() || !rest.contains('/') {
        (first, "", rest)
    } else {
        let (category, category_rest) = split_first(rest);
        (category, first, category_rest)
    };
    PatchSortMetadata {
        category: category.to_string(),
        source_rank: 1,
        vendor: vendor.to_string(),
        category_rest: category_rest.to_string(),
        path_rest: rest_path(path, "patches_3rdparty/"),
    }
}

fn rest_path(path: &str, prefix: &str) -> String {
    path.strip_prefix(prefix).unwrap_or(path).to_string()
}

fn vvp_sort_metadata(path: &str) -> PatchSortMetadata {
    let code = vvp_category_code(path);
    let category = cmrt_server_config::VAPORIZER2_CATEGORY_CODES
        .iter()
        .find(|(known, _)| code.eq_ignore_ascii_case(known))
        .map_or(code, |(_, name)| *name);
    PatchSortMetadata {
        category: category.to_string(),
        source_rank: 0,
        vendor: String::new(),
        category_rest: path.to_string(),
        path_rest: path.to_string(),
    }
}

pub(super) fn vvp_category_code(path: &str) -> &str {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file_name
        .rfind('.')
        .filter(|dot| *dot > 0)
        .map_or(file_name, |dot| &file_name[..dot]);
    let end = stem
        .char_indices()
        .nth(2)
        .map_or(stem.len(), |(end, _)| end);
    &stem[..end]
}
