use super::*;

const MINIMAL_CONFIG: &str = r#"
plugin_path = "/usr/lib/clap/Surge XT.clap"
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
chord_patch_categories = ["Pads"]
daw_tracks = 4
"#,
    );

    assert_eq!(cfg.plugin_path, "/usr/lib/clap/Surge XT.clap");
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

/// `active_plugin` の解決はサーバー側でも走る。ここが効かないと
/// `active_plugin = 'Dexed'` だけを書いた config でサーバーが Surge を読みに行く。
#[test]
fn an_active_plugin_is_baked_into_the_top_level_fields() {
    let cfg = load("active_plugin = 'Dexed'\n");

    assert_eq!(cfg.plugin_path, default_dexed_plugin_path());
    assert_eq!(cfg.plugin_id.as_deref(), Some(DEXED_PLUGIN_ID));
    assert_eq!(
        cfg.patches_dirs.as_deref().map(<[String]>::to_vec),
        Some(default_dexed_cartridge_dirs())
    );
}

#[test]
fn a_config_without_active_plugin_keeps_its_top_level_settings() {
    let cfg = load("patches_dirs = ['/surge/patches_factory']\n");

    assert_eq!(cfg.plugin_path, "/usr/lib/clap/Surge XT.clap");
    assert_eq!(cfg.plugin_id, None);
    assert_eq!(
        cfg.patches_dirs.as_deref().map(<[String]>::to_vec),
        Some(vec!["/surge/patches_factory".to_string()])
    );
}

#[test]
fn patch_root_dir_folds_the_configured_dirs() {
    let cfg = load("patches_dirs = ['/surge/patches_factory', '/surge/patches_3rdparty']\n");

    assert_eq!(cfg.patch_root_dir().as_deref(), Some("/surge"));
}

/// `[plugins.*]` に TUI 専用のカテゴリ設定が書かれていてもサーバーは読み飛ばす。
#[test]
fn patch_role_filters_inside_a_profile_do_not_break_the_server() {
    let cfg = load(
        r#"
active_plugin = 'Dexed'

[plugins.Dexed]
chord_patch_categories = ["SynprezFM"]
"#,
    );

    assert_eq!(cfg.plugin_id.as_deref(), Some(DEXED_PLUGIN_ID));
}
