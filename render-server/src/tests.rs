use super::*;

/// `ServerConfig` は増えるフィールドに serde default を付ける決まりなので、
/// 構造体リテラルではなく TOML から作って項目追加への追従を不要にする。
fn test_config() -> ServerConfig {
    ServerConfig::from_toml_str(
        r#"
output_midi = "output.mid"
output_wav = "output.wav"
sample_rate = 48000
buffer_size = 512
"#,
    )
    .unwrap()
}

#[test]
fn cli_without_subcommand_runs_server() {
    assert_eq!(
        parse_cli(["clap-mml-render-server"]).unwrap(),
        CliAction::Run { config: None }
    );
}

/// 実ユーザーの config.toml を書き換えずに `[plugins.*]` を試すための入口。
/// TUI 側の `cmrt patch-roles --config` / `cmrt render-mml --config` と対になる。
#[test]
fn cli_takes_an_optional_config_path() {
    assert_eq!(
        parse_cli(["clap-mml-render-server", "--config", "/tmp/try.toml"]).unwrap(),
        CliAction::Run {
            config: Some(PathBuf::from("/tmp/try.toml"))
        }
    );
}

#[test]
fn update_subcommand_returns_update_action() {
    assert_eq!(
        parse_cli(["clap-mml-render-server", "update"]).unwrap(),
        CliAction::Update
    );
}

#[test]
fn check_subcommand_returns_check_action() {
    assert_eq!(
        parse_cli(["clap-mml-render-server", "check"]).unwrap(),
        CliAction::Check
    );
}

#[test]
fn help_lists_self_update_commands_and_server_details() {
    let CliAction::PrintHelp(help) = parse_cli(["clap-mml-render-server", "--help"]).unwrap()
    else {
        panic!("expected help action");
    };

    assert!(help.contains("Commands:"));
    assert!(help.contains("update"));
    assert!(help.contains("check"));
    assert!(help.contains("POST /render"));
}

#[test]
fn unknown_argument_returns_error() {
    let error = parse_cli(["clap-mml-render-server", "unknown"]).unwrap_err();

    assert!(error
        .to_string()
        .contains("unrecognized subcommand 'unknown'"));
}

#[test]
fn core_config_from_server_config_uses_the_shared_patch_root() {
    let mut cfg = test_config();
    cfg.patches_dirs = Some(vec![
        "/tmp/surge-data/patches_factory".to_string(),
        "/tmp/surge-data/patches_3rdparty".to_string(),
    ]);

    let core_cfg = core_config_from_server_config(&cfg);

    assert_eq!(core_cfg.output_midi, "output.mid");
    assert_eq!(core_cfg.output_wav, "output.wav");
    assert_eq!(core_cfg.sample_rate, REQUIRED_SAMPLE_RATE);
    assert_eq!(core_cfg.buffer_size, 512);
    assert_eq!(core_cfg.patches_dir.as_deref(), Some("/tmp/surge-data"));
    assert!(!core_cfg.random_patch);
}

/// `plugin_id` を CoreConfig まで運べないと、descriptor を複数持つ CLAP で
/// 起動ログとレンダリング側の descriptor 選択が食い違う。
#[test]
fn core_config_from_server_config_carries_plugin_id() {
    let mut cfg = test_config();
    cfg.plugin_id = Some("com.digital-suburban.dexed".to_string());

    let core_cfg = core_config_from_server_config(&cfg);

    assert_eq!(
        core_cfg.plugin_id.as_deref(),
        Some("com.digital-suburban.dexed")
    );
}

#[test]
fn core_config_from_server_config_uses_the_builtin_surge_id_when_profile_omits_it() {
    let core_cfg = core_config_from_server_config(&test_config());

    assert_eq!(
        core_cfg.plugin_id.as_deref(),
        Some(cmrt_server_config::SURGE_XT_PLUGIN_ID)
    );
}

#[test]
fn render_server_plugin_kinds_retain_floe_as_a_distinct_form() {
    let root = std::env::temp_dir().join("cmrt_render_server_floe_kind");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("presets")).unwrap();
    std::fs::write(root.join("Floe.clap"), b"fixture").unwrap();
    let plugin = root.join("Floe.clap").to_string_lossy().replace('\\', "/");
    let presets = root.join("presets").to_string_lossy().replace('\\', "/");
    let cfg = ServerConfig::from_toml_str(&format!(
        r#"
output_midi = "output.mid"
output_wav = "output.wav"
sample_rate = 48000
buffer_size = 512

[plugins.Floe]
plugin_path = "{plugin}"
patches_dirs = ["{presets}"]
"#
    ))
    .unwrap();

    let kinds = plugin_kinds(&cfg, &core_config_from_server_config(&cfg));
    let floe = kinds.iter().find(|kind| kind.name == "Floe").unwrap();

    assert_eq!(floe.patch_form, cmrt_server_config::PatchForm::FloePreset);
    assert_eq!(
        floe.core_cfg.plugin_id.as_deref(),
        Some(cmrt_server_config::FLOE_PLUGIN_ID)
    );
    assert_eq!(floe.core_cfg.patches_dir.as_deref(), Some(presets.as_str()));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn validate_render_server_config_rejects_non_48khz() {
    let mut cfg = test_config();
    cfg.sample_rate = 44_100.0;

    let error = validate_render_server_config(&cfg).unwrap_err();

    assert!(error.to_string().contains("48000"));
}
