use super::*;

fn make_patches(items: &[&str]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|&s| (s.to_string(), s.to_lowercase()))
        .collect()
}

#[test]
fn filter_patches_empty_query_returns_all() {
    let all = make_patches(&["Pads/Pad 1.fxp", "Leads/Lead 1.fxp"]);
    let result = filter_patches(&all, "");
    assert_eq!(result, vec!["Pads/Pad 1.fxp", "Leads/Lead 1.fxp"]);
}

#[test]
fn filter_patches_single_term_matches_substring() {
    let all = make_patches(&["Pads/Pad 1.fxp", "Leads/Lead 1.fxp"]);
    let result = filter_patches(&all, "pad");
    assert_eq!(result, vec!["Pads/Pad 1.fxp"]);
}

#[test]
fn filter_patches_case_insensitive() {
    let all = make_patches(&["Pads/Pad 1.fxp", "Leads/Lead 1.fxp"]);
    let result = filter_patches(&all, "PAD");
    assert_eq!(result, vec!["Pads/Pad 1.fxp"]);
}

#[test]
fn filter_patches_multiple_terms_act_as_and() {
    let all = make_patches(&[
        "Pads/Soft Pad.fxp",
        "Pads/Hard Pad.fxp",
        "Leads/Lead 1.fxp",
    ]);
    let result = filter_patches(&all, "pad soft");
    assert_eq!(result, vec!["Pads/Soft Pad.fxp"]);
}

#[test]
fn filter_patches_no_match_returns_empty() {
    let all = make_patches(&["Pads/Pad 1.fxp"]);
    let result = filter_patches(&all, "xyznomatch");
    assert!(result.is_empty());
}

#[test]
fn filter_patches_whitespace_only_query_returns_all() {
    let all = make_patches(&["Pads/Pad 1.fxp", "Leads/Lead 1.fxp"]);
    // split_whitespace で空のイテレータになり、全件返す
    let result = filter_patches(&all, "   ");
    assert_eq!(result, vec!["Pads/Pad 1.fxp", "Leads/Lead 1.fxp"]);
}

#[test]
fn filter_patches_empty_list_returns_empty() {
    let all: Vec<(String, String)> = vec![];
    let result = filter_patches(&all, "pad");
    assert!(result.is_empty());
}

// --- audio cache helper tests ---

#[test]
fn resolve_cached_samples_returns_none_when_random_patch_true() {
    let mut cache = HashMap::new();
    cache.insert("cde".to_string(), vec![0.0f32, 1.0]);
    // random_patch=true: キャッシュが存在していてもNoneを返す
    let result = resolve_cached_samples(&cache, "cde", true);
    assert!(result.is_none());
}

#[test]
fn resolve_cached_samples_returns_samples_on_cache_hit() {
    let mut cache = HashMap::new();
    cache.insert("cde".to_string(), vec![0.5f32, 0.6]);
    let result = resolve_cached_samples(&cache, "cde", false);
    assert_eq!(result, Some(vec![0.5f32, 0.6]));
}

#[test]
fn resolve_cached_samples_returns_none_on_cache_miss() {
    let cache: HashMap<String, Vec<f32>> = HashMap::new();
    let result = resolve_cached_samples(&cache, "cde", false);
    assert!(result.is_none());
}

#[test]
fn try_insert_cache_does_nothing_when_random_patch_true() {
    let mut cache = HashMap::new();
    try_insert_cache(&mut cache, "cde".to_string(), vec![1.0f32], true);
    assert!(cache.is_empty());
}

#[test]
fn try_insert_cache_inserts_when_random_patch_false() {
    let mut cache = HashMap::new();
    try_insert_cache(&mut cache, "cde".to_string(), vec![1.0f32], false);
    assert!(cache.contains_key("cde"));
}

#[test]
fn try_insert_cache_clears_and_inserts_when_full() {
    let mut cache = HashMap::new();
    // AUDIO_CACHE_MAX_ENTRIES まで埋める
    for i in 0..AUDIO_CACHE_MAX_ENTRIES {
        cache.insert(format!("mml_{}", i), vec![]);
    }
    assert_eq!(cache.len(), AUDIO_CACHE_MAX_ENTRIES);

    // 新しいキーの挿入でクリアが起きる
    try_insert_cache(&mut cache, "new_mml".to_string(), vec![0.1f32], false);
    // クリア後に1エントリだけ残る
    assert_eq!(cache.len(), 1);
    assert!(cache.contains_key("new_mml"));
}

#[test]
fn try_insert_cache_updates_existing_key_when_full() {
    let mut cache = HashMap::new();
    // "cde" を含めてちょうど AUDIO_CACHE_MAX_ENTRIES 件になるよう埋める
    for i in 0..(AUDIO_CACHE_MAX_ENTRIES - 1) {
        cache.insert(format!("mml_{}", i), vec![]);
    }
    cache.insert("cde".to_string(), vec![]);
    assert_eq!(cache.len(), AUDIO_CACHE_MAX_ENTRIES);

    // 上限ちょうどの状態で既存キーを更新してもクリアは発生しない
    try_insert_cache(&mut cache, "cde".to_string(), vec![0.9f32], false);
    assert_eq!(cache.len(), AUDIO_CACHE_MAX_ENTRIES);
    assert_eq!(cache["cde"], vec![0.9f32]);
}
