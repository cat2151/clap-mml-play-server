use super::*;
use crate::{default_patches_dirs, SURGE_XT_PLUGIN_ID};

#[test]
fn a_config_without_retired_top_level_keys_is_allowed() {
    reject_retired_top_level_plugin_keys(
        r#"
output_midi = "output.mid"
[plugins."Surge XT"]
patches_dirs = ["/surge/patches"]
"#,
    )
    .unwrap();
}

#[test]
fn every_retired_top_level_key_is_rejected_by_name() {
    let values = [
        ("active_plugin", "'Surge XT'"),
        ("plugin_path", "'/clap/Surge XT.clap'"),
        ("plugin_id", "'org.surge-synth-team.surge-xt'"),
        ("patches_dirs", "['/surge/patches']"),
        ("chord_patch_categories", "['Pads']"),
        ("bass_patch_categories", "['Basses']"),
        ("arpeggio_patch_categories", "['Leads']"),
        ("drum_patch_categories", "['Drums']"),
        ("kick_patch_keywords", "['kick']"),
        ("snare_patch_keywords", "['snare']"),
        ("hihat_patch_keywords", "['hat']"),
    ];

    for (key, value) in values {
        let error = reject_retired_top_level_plugin_keys(&format!("{key} = {value}\n"))
            .expect_err("トップレベルの旧設定は拒否する");
        let message = error.to_string();
        assert!(message.contains(key), "{message}");
        assert!(message.contains(r#"[plugins."Surge XT"]"#), "{message}");
    }
}

#[test]
fn surge_profile_settings_are_not_mistaken_for_top_level_keys() {
    reject_retired_top_level_plugin_keys(
        r#"
[plugins."Surge XT"]
plugin_path = "/opt/clap/Surge XT.clap"
plugin_id = "org.surge-synth-team.surge-xt"
patches_dirs = ["/opt/surge/patches"]
"#,
    )
    .unwrap();
}

#[test]
fn the_primary_plugin_is_always_surge_xt() {
    let profile = resolve_primary_plugin_profile(&BTreeMap::new()).unwrap();

    assert_eq!(profile.plugin_id.as_deref(), Some(SURGE_XT_PLUGIN_ID));
    assert_eq!(profile.patches_dirs, Some(default_patches_dirs()));
}

#[test]
fn a_surge_xt_profile_overrides_the_builtin_values() {
    let configured = toml::from_str::<Profiles>(
        r#"
[plugins.SurgeXT]
plugin_path = "/opt/clap/Surge XT.clap"
patches_dirs = ["/opt/surge/patches"]
"#,
    )
    .unwrap();

    let profile = resolve_primary_plugin_profile(&configured.plugins).unwrap();

    assert_eq!(profile.plugin_path, "/opt/clap/Surge XT.clap");
    assert_eq!(
        profile.patches_dirs,
        Some(vec!["/opt/surge/patches".to_string()])
    );
    assert_eq!(profile.plugin_id.as_deref(), Some(SURGE_XT_PLUGIN_ID));
}

#[derive(serde::Deserialize)]
struct Profiles {
    plugins: BTreeMap<String, PluginProfile>,
}
