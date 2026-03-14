use super::*;

#[test]
fn config_file_path_ends_with_cmrt_config_toml() {
    if let Some(path) = config_file_path() {
        let path_str = path.to_string_lossy();
        assert!(
            path_str.ends_with("cmrt/config.toml") || path_str.ends_with(r"cmrt\config.toml"),
            "config_file_path が cmrt/config.toml で終わっていない: {}",
            path_str
        );
    }
    // dirs::config_dir() が None の環境ではテストをスキップする
}

#[test]
fn config_file_path_contains_cmrt_subdir() {
    if let Some(path) = config_file_path() {
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("cmrt"),
            "config_file_path に cmrt が含まれていない: {}",
            path_str
        );
    }
}

#[test]
fn config_parse_valid_toml() {
    let toml_str = r#"
plugin_path = "/usr/lib/clap/Surge XT.clap"
input_midi  = "input.mid"
output_midi = "output.mid"
output_wav  = "output.wav"
sample_rate = 44100
buffer_size = 512
"#;
    let cfg: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.plugin_path, "/usr/lib/clap/Surge XT.clap");
    assert_eq!(cfg.output_midi, "output.mid");
    assert_eq!(cfg.output_wav, "output.wav");
    assert!((cfg.sample_rate - 44100.0).abs() < f64::EPSILON);
    assert_eq!(cfg.buffer_size, 512);
    assert!(cfg.random_patch); // デフォルトは true
}

#[test]
fn config_random_patch_defaults_to_true() {
    let toml_str = r#"
plugin_path = "/usr/lib/clap/Surge XT.clap"
input_midi  = "input.mid"
output_midi = "output.mid"
output_wav  = "output.wav"
sample_rate = 44100
buffer_size = 512
"#;
    let cfg: Config = toml::from_str(toml_str).unwrap();
    assert!(cfg.random_patch, "random_patch のデフォルトは true であるべき");
}

#[test]
fn config_random_patch_can_be_set_false() {
    let toml_str = r#"
plugin_path = "/usr/lib/clap/Surge XT.clap"
input_midi  = "input.mid"
output_midi = "output.mid"
output_wav  = "output.wav"
sample_rate = 44100
buffer_size = 512
random_patch = false
"#;
    let cfg: Config = toml::from_str(toml_str).unwrap();
    assert!(!cfg.random_patch);
}

#[test]
fn config_optional_patch_path_is_none_by_default() {
    let toml_str = r#"
plugin_path = "/usr/lib/clap/Surge XT.clap"
input_midi  = "input.mid"
output_midi = "output.mid"
output_wav  = "output.wav"
sample_rate = 44100
buffer_size = 512
"#;
    let cfg: Config = toml::from_str(toml_str).unwrap();
    assert!(cfg.patch_path.is_none());
    assert!(cfg.patches_dir.is_none());
}
