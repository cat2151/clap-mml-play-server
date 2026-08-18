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

#[derive(Deserialize, Default)]
struct PluginsToml {
    #[serde(default)]
    plugins: BTreeMap<String, PluginProfile>,
}

fn profiles(toml_str: &str) -> BTreeMap<String, PluginProfile> {
    toml::from_str::<PluginsToml>(toml_str).unwrap().plugins
}

/// `[plugins.*]` を書いた config での解決。トップレベル `plugin_path` は無しの想定。
fn resolve(active: &str, toml_str: &str) -> anyhow::Result<PluginProfile> {
    resolve_active_plugin_profile(Some(active), &profiles(toml_str), "")
        .map(|profile| profile.expect("active_plugin を渡したので解決されるはず"))
}

/// `[plugins.*]` が 1 つも無い config での解決（組み込みプロファイルだけを使う）。
fn resolve_builtin(active: &str) -> anyhow::Result<PluginProfile> {
    resolve(active, "")
}

#[test]
fn a_config_without_active_plugin_resolves_to_nothing() {
    let resolved =
        resolve_active_plugin_profile(None, &profiles(SURGE_AND_DEXED_PROFILES), "/clap/x.clap")
            .unwrap();

    assert_eq!(resolved, None);
}

#[test]
fn an_active_profile_is_resolved_from_the_plugins_table() {
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
fn switching_the_active_profile_switches_the_patch_directories() {
    let profile = resolve("surge_xt", SURGE_AND_DEXED_PROFILES).unwrap();

    assert_eq!(profile.plugin_path, "/clap/Surge XT.clap");
    assert_eq!(
        configured_patch_dirs(profile.patches_dirs.as_deref()).len(),
        2
    );
}

/// 移行の途中で必ず引っかかるので、トップレベル `plugin_path` との併記は
/// conflict error にしない（stderr へ「無視します」と出すだけ）。
#[test]
fn a_profile_wins_over_a_top_level_plugin_path_without_erroring() {
    let profile = resolve_active_plugin_profile(
        Some("dexed"),
        &profiles(SURGE_AND_DEXED_PROFILES),
        "/clap/Surge XT.clap",
    )
    .unwrap()
    .unwrap();

    assert_eq!(profile.plugin_path, "/clap/Dexed.clap");
}

#[test]
fn an_active_plugin_without_a_matching_profile_lists_the_available_names() {
    let error = resolve("dxd", SURGE_AND_DEXED_PROFILES).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("dxd"));
    // 組み込みの名前も config の名前も、両方を示す。
    assert!(message.contains("Dexed"));
    assert!(message.contains("Surge XT"));
    assert!(message.contains("surge_xt"));
}

#[test]
fn an_active_profile_with_an_empty_plugin_path_is_an_error() {
    let error = resolve(
        "broken",
        r#"
[plugins.broken]
plugin_path = "   "
"#,
    )
    .unwrap_err();

    assert!(error.to_string().contains("plugin_path"));
}

#[test]
fn an_empty_active_plugin_name_is_an_error() {
    let error = resolve_active_plugin_profile(Some("   "), &BTreeMap::new(), "").unwrap_err();

    assert!(error.to_string().contains("active_plugin"));
}

/// ユーザーが最初に書く形。`[plugins.*]` が 1 つも無くても動くことがこの機能の要。
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

/// Dexed の cartridge には Surge のようなカテゴリ階層が無いので、組み込みプロファイルは
/// 用途別の絞り込みを全て外す。ここが効かないと TUI の chord / bass / drum 行の候補が
/// 0 件になる（焼き込みは TUI 側 `cmrt_runtime` の担当）。
#[test]
fn the_builtin_dexed_profile_does_not_narrow_the_patch_roles() {
    let profile = resolve_builtin("Dexed").unwrap();

    assert_eq!(profile.patch_roles, PatchRoleFilters::unfiltered());
}

/// Surge のプロファイルは絞り込みを 1 つも書かない（＝ TUI のトップレベル設定が残る）。
/// 既存 config を持つ Surge ユーザーの挙動が変わらないことの担保。
#[test]
fn the_builtin_surge_profile_writes_no_patch_role_filters() {
    let profile = resolve_builtin("Surge XT").unwrap();

    assert_eq!(profile.patch_roles, PatchRoleFilters::default());
}

/// プロファイル側にカテゴリを書けば、そのプラグインだけ絞り込める。
/// 書かなかった項目は組み込みの「絞らない」が残る。
#[test]
fn a_profile_can_narrow_the_patch_roles_by_itself() {
    let profile = resolve(
        "Dexed",
        r#"
[plugins.Dexed]
chord_patch_categories = ["SynprezFM"]
"#,
    )
    .unwrap();

    assert_eq!(
        profile.patch_roles.chord_patch_categories,
        Some(vec!["SynprezFM".to_string()])
    );
    assert_eq!(
        profile.patch_roles.bass_patch_categories,
        Some(Vec::<String>::new())
    );
}
