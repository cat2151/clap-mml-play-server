use super::*;
use crate::configured_patch_dirs;

const SURGE_AND_DEXED_PROFILES: &str = r#"
[plugins.surge_xt]
plugin_path = "/clap/Surge XT.clap"
plugin_id   = "org.surge-synth-team.surge-xt"
patches_dirs = ["/surge/patches_factory", "/surge/patches_3rdparty"]

[plugins.dexed]
plugin_path = "/clap/Dexed.clap"
plugin_id   = "com.digital-suburban.dexed"
"#;

#[derive(Debug, Deserialize, Default)]
struct PluginsToml {
    #[serde(default)]
    plugins: BTreeMap<String, PluginProfile>,
}

fn profiles(toml_str: &str) -> BTreeMap<String, PluginProfile> {
    toml::from_str::<PluginsToml>(toml_str).unwrap().plugins
}

/// 組み込み値と `[plugins.*]` の merge 結果から名前で引く。
fn resolve(name: &str, toml_str: &str) -> anyhow::Result<PluginProfile> {
    lookup(&merged_plugin_profiles(&profiles(toml_str)), name)
        .ok_or_else(|| anyhow::anyhow!("plugin profile がありません: {name}"))
}

/// `[plugins.*]` が 1 つも無い config での解決（組み込みプロファイルだけを使う）。
fn resolve_builtin(name: &str) -> anyhow::Result<PluginProfile> {
    resolve(name, "")
}

#[test]
fn a_profile_is_resolved_from_the_plugins_table() {
    let profile = resolve("dexed", SURGE_AND_DEXED_PROFILES).unwrap();

    assert_eq!(profile.plugin_path, "/clap/Dexed.clap");
    assert_eq!(
        profile.plugin_id.as_deref(),
        Some("com.digital-suburban.dexed")
    );
}

/// プロファイルに `patches_dirs` を書かなければ組み込みの値が残る。
/// 他プロファイルや旧トップレベルの Surge 用ディレクトリを流用してはいけない。
#[test]
fn a_profile_without_patches_dirs_falls_back_to_the_builtin_ones() {
    let profile = resolve("dexed", SURGE_AND_DEXED_PROFILES).unwrap();

    let dirs = configured_patch_dirs(profile.patches_dirs.as_deref());
    assert_eq!(dirs, default_dexed_cartridge_dirs());
    assert!(!dirs.iter().any(|dir| dir.contains("surge")));
}

#[test]
fn each_profile_keeps_its_own_patch_directories() {
    let profile = resolve("surge_xt", SURGE_AND_DEXED_PROFILES).unwrap();

    assert_eq!(profile.plugin_path, "/clap/Surge XT.clap");
    assert_eq!(
        configured_patch_dirs(profile.patches_dirs.as_deref()).len(),
        2
    );
}

/// `[plugins.*]` が 1 つも無くても組み込みプロファイルを取得できる。
#[test]
fn a_builtin_name_alone_needs_no_plugins_table() {
    let profile = resolve_builtin("Dexed").unwrap();

    assert_eq!(profile.plugin_path, default_dexed_plugin_path());
    assert_eq!(
        profile.plugin_id.as_deref(),
        Some("com.digital-suburban.dexed")
    );
    // Dexed が factory cartridge を展開する場所が組み込みで入る。
    assert_eq!(
        configured_patch_dirs(profile.patches_dirs.as_deref()),
        default_dexed_cartridge_dirs()
    );
}

#[test]
fn the_builtin_surge_profile_brings_its_patch_directories() {
    let profile = resolve_builtin("Surge XT").unwrap();

    assert_eq!(profile.plugin_path, default_plugin_path());
    assert_eq!(
        profile.plugin_id.as_deref(),
        Some("org.surge-synth-team.surge-xt")
    );
    assert_eq!(
        configured_patch_dirs(profile.patches_dirs.as_deref()),
        default_patches_dirs()
    );
}

/// 大文字小文字・空白・アンダースコアの違いで起動できなくなるのは事故のもと。
#[test]
fn builtin_names_ignore_case_spaces_and_underscores() {
    for name in ["dexed", "DEXED", "De xed"] {
        let profile = resolve_builtin(name).unwrap();
        assert_eq!(
            profile.plugin_id.as_deref(),
            Some("com.digital-suburban.dexed"),
            "{name}"
        );
    }
    for name in ["surge_xt", "surge xt", "SurgeXT"] {
        let profile = resolve_builtin(name).unwrap();
        assert_eq!(
            profile.plugin_id.as_deref(),
            Some("org.surge-synth-team.surge-xt"),
            "{name}"
        );
    }
}

/// 標準以外の場所に入れている人は plugin_path だけ書けばよく、
/// plugin_id や patches_dirs を書き写す必要はない。
#[test]
fn a_configured_profile_overrides_only_the_fields_it_writes() {
    let profile = resolve(
        "Surge XT",
        r#"
[plugins."Surge XT"]
plugin_path = "/opt/clap/Surge XT.clap"
"#,
    )
    .unwrap();

    assert_eq!(profile.plugin_path, "/opt/clap/Surge XT.clap");
    assert_eq!(
        profile.plugin_id.as_deref(),
        Some("org.surge-synth-team.surge-xt")
    );
    assert_eq!(
        configured_patch_dirs(profile.patches_dirs.as_deref()),
        default_patches_dirs()
    );
}

/// 組み込みの `patches_dirs` を消したいときは、明示的に空配列を書く。
#[test]
fn an_empty_patches_dirs_clears_the_builtin_ones() {
    let profile = resolve(
        "Surge XT",
        r#"
[plugins."Surge XT"]
patches_dirs = []
"#,
    )
    .unwrap();

    assert_eq!(profile.plugin_path, default_plugin_path());
    assert!(configured_patch_dirs(profile.patches_dirs.as_deref()).is_empty());
}

/// 組み込みと同名の profile を config に書いても、既存の書き方（全項目を書く）は壊れない。
#[test]
fn a_fully_written_profile_still_wins_over_the_builtin() {
    let profile = resolve(
        "Dexed",
        r#"
[plugins.Dexed]
plugin_path = "/opt/clap/Dexed.clap"
plugin_id = "custom.dexed"
"#,
    )
    .unwrap();

    assert_eq!(profile.plugin_path, "/opt/clap/Dexed.clap");
    assert_eq!(profile.plugin_id.as_deref(), Some("custom.dexed"));
}

/// Vaporizer2 も名前 1 行で使える。**ただし音色置き場は組み込みでは埋まらない。**
/// プリセット置き場はユーザーが決めるものなので、config に書いてもらう
/// （書かなければ音色置き場が空のままカタログに載らない、という安全側の倒れ方をする）。
#[test]
fn the_builtin_vaporizer2_profile_brings_no_patch_directories() {
    let profile = resolve_builtin("Vaporizer2").unwrap();

    assert_eq!(profile.plugin_path, default_vaporizer2_plugin_path());
    assert_eq!(profile.plugin_id.as_deref(), Some("com.vastdynamics.VAST2"));
    assert_eq!(profile.patches_dirs, None);
    assert!(configured_patch_dirs(profile.patches_dirs.as_deref()).is_empty());
}

/// 標準の場所へ入れているユーザーが書くのは `patches_dirs` の 1 行だけで済む。
#[test]
fn a_vaporizer2_profile_only_needs_its_patches_dirs() {
    let profile = resolve(
        "Vaporizer2",
        r#"
[plugins.Vaporizer2]
patches_dirs = ["/presets/Vaporizer2"]
"#,
    )
    .unwrap();

    assert_eq!(profile.plugin_path, default_vaporizer2_plugin_path());
    assert_eq!(profile.plugin_id.as_deref(), Some("com.vastdynamics.VAST2"));
    assert_eq!(
        configured_patch_dirs(profile.patches_dirs.as_deref()),
        vec!["/presets/Vaporizer2".to_string()]
    );
}

#[test]
fn the_builtin_floe_profile_has_identity_but_no_patch_directories() {
    let profile = resolve_builtin("Floe").unwrap();

    assert_eq!(profile.plugin_path, default_floe_plugin_path());
    assert_eq!(profile.plugin_id.as_deref(), Some(FLOE_PLUGIN_ID));
    assert_eq!(profile.patches_dirs, None);
}

#[test]
fn the_builtin_sforzando_profile_has_identity() {
    let profile = resolve_builtin("Sforzando").unwrap();

    assert_eq!(profile.plugin_path, default_sforzando_plugin_path());
    assert_eq!(profile.plugin_id.as_deref(), Some(SFORZANDO_PLUGIN_ID));
    assert_eq!(profile.patches_dirs, None);
}

#[test]
fn a_floe_profile_only_needs_its_patches_dirs() {
    let profile = resolve(
        "Floe",
        r#"
[plugins.Floe]
patches_dirs = ["/presets/Floe"]
"#,
    )
    .unwrap();

    assert_eq!(profile.plugin_path, default_floe_plugin_path());
    assert_eq!(profile.plugin_id.as_deref(), Some(FLOE_PLUGIN_ID));
    assert_eq!(
        configured_patch_dirs(profile.patches_dirs.as_deref()),
        vec!["/presets/Floe".to_string()]
    );
}

/// 旧Role設定は移行期間なしで廃止し、未知キーとして明示的に拒否する。
#[test]
fn retired_patch_role_keys_are_rejected() {
    let error = toml::from_str::<PluginsToml>(
        r#"
[plugins.Dexed]
chord_patch_categories = ["SynprezFM"]
"#,
    )
    .unwrap_err();

    assert!(format!("{error:#}").contains("chord_patch_categories"));
}

/// `plugin_id` が書いてあるなら、それだけで形が決まること。
#[test]
fn the_plugin_id_decides_the_patch_form() {
    assert_eq!(
        patch_form_of(Some(SURGE_XT_PLUGIN_ID), "whatever.clap"),
        PatchForm::StateFile
    );
    assert_eq!(
        patch_form_of(Some(DEXED_PLUGIN_ID), "whatever.clap"),
        PatchForm::Cartridge
    );
    assert_eq!(
        patch_form_of(Some(VAPORIZER2_PLUGIN_ID), "whatever.clap"),
        PatchForm::Vvp
    );
    assert_eq!(
        patch_form_of(Some(FLOE_PLUGIN_ID), "whatever.clap"),
        PatchForm::FloePreset
    );
    assert_eq!(
        patch_form_of(Some(SFORZANDO_PLUGIN_ID), "whatever.clap"),
        PatchForm::Sfz
    );
}

/// `plugin_id` を書いていない config でも、ファイル名から拾えること。
/// 実ファイル名は `VASTvaporizer2.clap` なので、大文字小文字を無視して照合する。
#[test]
fn the_file_name_is_the_last_resort_when_no_plugin_id_is_written() {
    assert_eq!(
        patch_form_of(None, r"C:\CLAP\VASTvaporizer2.clap"),
        PatchForm::Vvp
    );
    assert_eq!(
        patch_form_of(None, r"C:\CLAP\Dexed.clap"),
        PatchForm::Cartridge
    );
    assert_eq!(
        patch_form_of(None, r"C:\CLAP\Surge XT.clap"),
        PatchForm::StateFile
    );
    assert_eq!(
        patch_form_of(None, r"C:\CLAP\FLOE.clap"),
        PatchForm::FloePreset
    );
    assert_eq!(
        patch_form_of(None, r"C:\CLAP\sforzando_x64.clap"),
        PatchForm::Sfz
    );
}

/// 知らないプラグインは `StateFile` へ落とす。`.vvp` / `.syx` を読む CLAP は
/// 実質 1 つずつしかないので、既定は Surge と同じ形のほうが当たる見込みが高い。
#[test]
fn an_unknown_plugin_still_falls_back_to_the_state_file_form() {
    assert_eq!(
        patch_form_of(Some("com.example.unknown"), "Unknown.clap"),
        PatchForm::StateFile
    );
}
