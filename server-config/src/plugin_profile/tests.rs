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

/// Surge と同じく絞り込みを 1 つも書かない。用途別カテゴリの実データは TUI 側の担当で、
/// ここへ書くと play server → TUI の逆向き依存が復活する。
#[test]
fn the_builtin_vaporizer2_profile_writes_no_patch_role_filters() {
    let profile = resolve_builtin("Vaporizer2").unwrap();

    assert_eq!(profile.patch_roles, PatchRoleFilters::default());
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

/// 3 つ目の組み込み名が、名前を間違えたときの案内にも出ること。
#[test]
fn the_available_names_now_list_three_builtins() {
    let error = resolve("vaporiser2", "").unwrap_err();

    let message = error.to_string();
    assert!(message.contains("Surge XT"), "{message}");
    assert!(message.contains("Dexed"), "{message}");
    assert!(message.contains("Vaporizer2"), "{message}");
}

#[test]
fn the_vaporizer2_builtin_name_ignores_case_and_spaces() {
    for name in ["vaporizer2", "VAPORIZER2", "Vaporizer 2", "vaporizer_2"] {
        let profile = resolve_builtin(name).unwrap();
        assert_eq!(
            profile.plugin_id.as_deref(),
            Some("com.vastdynamics.VAST2"),
            "{name}"
        );
    }
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
