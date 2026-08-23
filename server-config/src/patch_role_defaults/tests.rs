use super::*;
use crate::{DEXED_PLUGIN_ID, VAPORIZER2_PLUGIN_ID};

#[test]
fn concrete_taxonomies_are_resolved_to_abstract_filters() {
    let surge = builtin_patch_role_filters(Some(SURGE_XT_PLUGIN_ID), "ignored");
    assert_eq!(
        surge.chord_patch_categories.unwrap(),
        owned(&SURGE_CHORD_CATEGORIES)
    );

    let vaporizer = builtin_patch_role_filters(Some(VAPORIZER2_PLUGIN_ID), "ignored");
    assert_eq!(
        vaporizer.bass_patch_categories.unwrap(),
        owned(&VAPORIZER2_BASS_CATEGORIES)
    );
}

#[test]
fn plugins_without_a_taxonomy_are_unfiltered() {
    assert_eq!(
        builtin_patch_role_filters(Some(DEXED_PLUGIN_ID), "ignored"),
        PatchRoleFilters::unfiltered()
    );
    assert_eq!(
        builtin_patch_role_filters(Some("example.unknown"), "Unknown.clap"),
        PatchRoleFilters::unfiltered()
    );
}
