use super::*;

#[test]
fn obxf_profile_shares_the_fxp_form_but_has_its_own_tag_and_roots() {
    let profile = resolve_builtin("OB-Xf").unwrap();
    assert_eq!(profile.plugin_id.as_deref(), Some(crate::OBXF_PLUGIN_ID));
    assert_eq!(
        profile.patches_dirs,
        Some(plugin_presets::obxf::obxf_preset_dirs())
    );
    assert_eq!(
        patch_form_of(profile.plugin_id.as_deref(), &profile.plugin_path),
        PatchForm::StateFile
    );
    assert_eq!(
        crate::patch_tag_of(profile.plugin_id.as_deref(), &profile.plugin_path),
        crate::OBXF_PATCH_TAG
    );
    assert_eq!(
        crate::patch_tag_of(None, "X:/plugins/OB-Xf.clap"),
        crate::OBXF_PATCH_TAG
    );
}
