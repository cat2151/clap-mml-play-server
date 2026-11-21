use super::*;

#[test]
fn apricot_uses_its_native_user_folder_and_a_distinct_patch_form() {
    let profile = resolve(
        "Apricot",
        r#"
[plugins.apricot]
plugin_path = "X:/plugins/Apricot.clap"
patches_dirs = ["X:/unrelated"]
"#,
    )
    .unwrap();
    assert_eq!(profile.plugin_id.as_deref(), Some(crate::APRICOT_PLUGIN_ID));
    assert_eq!(
        profile.patches_dirs.unwrap(),
        plugin_presets::apricot::apricot_preset_dirs()
    );
    assert_eq!(
        patch_form_of(Some(crate::APRICOT_PLUGIN_ID), "X:/renamed.clap"),
        PatchForm::ApricotPreset
    );
}
