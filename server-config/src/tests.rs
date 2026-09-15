use super::*;

const MINIMAL_CONFIG: &str = r#"
output_midi = "output.mid"
output_wav  = "output.wav"
sample_rate = 48000
buffer_size = 512
"#;

fn load(extra: &str) -> ServerConfig {
    ServerConfig::from_toml_str(&format!("{MINIMAL_CONFIG}{extra}")).unwrap()
}

#[test]
fn ports_and_workers_fall_back_to_the_defaults() {
    let cfg = load("");

    assert_eq!(
        cfg.offline_render_server_workers,
        DEFAULT_OFFLINE_RENDER_SERVER_WORKERS
    );
    assert_eq!(
        cfg.offline_render_server_port,
        DEFAULT_OFFLINE_RENDER_SERVER_PORT
    );
    assert_eq!(
        cfg.realtime_play_server_port,
        DEFAULT_REALTIME_PLAY_SERVER_PORT
    );
}

#[test]
fn app_http_base_url_uses_the_shared_default_port() {
    assert!(DEFAULT_APP_HTTP_BASE_URL.ends_with(&format!(":{DEFAULT_APP_HTTP_SERVER_PORT}")));
}

#[test]
fn default_ports_stay_below_the_windows_default_dynamic_range() {
    for port in [
        DEFAULT_APP_HTTP_SERVER_PORT,
        DEFAULT_OFFLINE_RENDER_SERVER_PORT,
        DEFAULT_REALTIME_PLAY_SERVER_PORT,
    ] {
        assert!(
            port < 49_152,
            "default server port {port} overlaps the Windows default dynamic range"
        );
    }
}

#[test]
fn explicit_ports_and_workers_are_read() {
    let cfg = load(
        r#"
offline_render_server_workers = 8
offline_render_server_port = 62253
realtime_play_server_port = 62254
"#,
    );

    assert_eq!(cfg.offline_render_server_workers, 8);
    assert_eq!(cfg.offline_render_server_port, 62253);
    assert_eq!(cfg.realtime_play_server_port, 62254);
}

/// TUI 固有の項目（`loop_dirs` など）は同じ config.toml に必ず並んでいる。
/// サーバーが「知らないキーがある」で落ちてはいけない。
#[test]
fn tui_only_keys_are_ignored() {
    let cfg = load(
        r#"
input_midi = "input.mid"
loop_dirs = ["/tmp/loops"]
loop_categories = ["guitar"]
autoplay_on_startup = false
daw_tracks = 4

"#,
    );

    assert_eq!(cfg.plugin_path, default_plugin_path());
}

#[test]
fn zero_ports_are_rejected() {
    for key in ["offline_render_server_port", "realtime_play_server_port"] {
        let error =
            ServerConfig::from_toml_str(&format!("{MINIMAL_CONFIG}{key} = 0\n")).unwrap_err();

        assert!(format!("{error:#}").contains(key), "{key}");
    }
}

#[test]
fn out_of_range_workers_are_rejected() {
    for workers in ["0", "17"] {
        let error = ServerConfig::from_toml_str(&format!(
            "{MINIMAL_CONFIG}offline_render_server_workers = {workers}\n"
        ))
        .unwrap_err();

        assert!(
            format!("{error:#}").contains("offline_render_server_workers"),
            "{workers}"
        );
    }
}

#[test]
fn surge_xt_is_baked_into_the_runtime_fields() {
    let cfg = load("");

    assert_eq!(cfg.plugin_path, default_plugin_path());
    assert_eq!(cfg.plugin_id.as_deref(), Some(SURGE_XT_PLUGIN_ID));
    assert_eq!(
        cfg.patches_dirs.as_deref().map(<[String]>::to_vec),
        Some(default_patches_dirs())
    );
}

#[test]
fn a_surge_xt_profile_overrides_the_runtime_fields() {
    let cfg = load(
        r#"
[plugins."Surge XT"]
plugin_path = "/opt/clap/Surge XT.clap"
patches_dirs = ["/surge/patches_factory"]
"#,
    );

    assert_eq!(cfg.plugin_path, "/opt/clap/Surge XT.clap");
    assert_eq!(cfg.plugin_id.as_deref(), Some(SURGE_XT_PLUGIN_ID));
    assert_eq!(
        cfg.patches_dirs.as_deref().map(<[String]>::to_vec),
        Some(vec!["/surge/patches_factory".to_string()])
    );
}

#[test]
fn retired_top_level_plugin_settings_are_rejected() {
    for line in [
        "active_plugin = 'Surge XT'",
        "plugin_path = '/clap/Surge XT.clap'",
        "plugin_id = 'org.surge-synth-team.surge-xt'",
        "patches_dirs = ['/surge/patches_factory']",
        "chord_patch_categories = ['Pads']",
    ] {
        let error = ServerConfig::from_toml_str(&format!("{MINIMAL_CONFIG}{line}\n"))
            .expect_err("廃止したトップレベル設定は拒否する");
        let message = format!("{error:#}");
        assert!(
            message.contains(line.split_once(' ').unwrap().0),
            "{message}"
        );
    }
}

#[test]
fn patch_root_dir_folds_the_configured_dirs() {
    let cfg = load(
        r#"
[plugins."Surge XT"]
patches_dirs = ["/surge/patches_factory", "/surge/patches_3rdparty"]
"#,
    );

    assert_eq!(cfg.patch_root_dir().as_deref(), Some("/surge"));
}

/// 旧Role設定はprofile内でも黙って無視せず拒否する。
#[test]
fn patch_role_filters_inside_a_profile_are_rejected() {
    let error = ServerConfig::from_toml_str(&format!(
        "{MINIMAL_CONFIG}{}",
        r#"
[plugins.Dexed]
chord_patch_categories = ["SynprezFM"]
"#,
    ))
    .unwrap_err();

    assert!(format!("{error:#}").contains("chord_patch_categories"));
}

#[test]
fn patch_dirs_of_reads_the_profile_and_drops_blank_entries() {
    let cfg = load(
        r#"
[plugins.Vaporizer2]
patches_dirs = ["/vaporizer2/presets", ""]
"#,
    );

    assert_eq!(
        cfg.patch_dirs_of("Vaporizer2"),
        vec!["/vaporizer2/presets".to_string()]
    );
}

#[test]
fn patch_dirs_of_is_empty_when_neither_config_nor_builtin_has_one() {
    let cfg = load("");

    assert_eq!(cfg.patch_dirs_of("Vaporizer2"), Vec::<String>::new());
    assert_eq!(cfg.patch_dirs_of("no such plugin"), Vec::<String>::new());
}

/// config に節が無くても、組み込みプロファイルの既定値は返る（Dexed の cartridge 置き場）。
#[test]
fn patch_dirs_of_falls_back_to_the_builtin_profile() {
    let cfg = load("");

    assert_eq!(cfg.patch_dirs_of("Dexed"), default_dexed_cartridge_dirs());
}
