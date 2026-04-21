use super::*;

#[test]
fn write_wav_creates_valid_riff_file() {
    let path = std::env::temp_dir().join("cmrt_test_write_wav.wav");
    let path_str = path.to_str().unwrap();
    // ステレオ2フレーム分のサンプル（L,R,L,R）
    let samples: Vec<f32> = vec![0.0, 0.0, 0.1, -0.1];
    write_wav(&samples, 44100, path_str).unwrap();

    let content = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();

    // WAV ファイルは "RIFF" で始まる
    assert!(
        content.starts_with(b"RIFF"),
        "WAV ファイルが RIFF ヘッダで始まっていない"
    );
    // 最低限ヘッダ (44 bytes) 以上のサイズがある
    assert!(content.len() > 44);
}

#[test]
fn write_wav_empty_samples_creates_valid_file() {
    let path = std::env::temp_dir().join("cmrt_test_write_wav_empty.wav");
    let path_str = path.to_str().unwrap();
    let samples: Vec<f32> = vec![];
    write_wav(&samples, 44100, path_str).unwrap();

    let content = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();

    assert!(content.starts_with(b"RIFF"));
}

#[test]
fn write_wav_invalid_path_returns_error() {
    let samples: Vec<f32> = vec![0.0, 0.0];
    let result = write_wav(&samples, 44100, "/nonexistent/directory/file.wav");
    assert!(result.is_err());
}

#[test]
fn mml_str_to_smf_bytes_returns_valid_smf() {
    // "cde" → ドレミ3音の SMF バイト列が生成されることを確認する
    // 中間ファイル（pass1_tokens.json 等）が config_local_dir()/clap-mml-render-tui/ に書き出されるが、
    // 戻り値の計算自体はメモリ上で行われるため機能テストとして有効
    // CMRT_BASE_DIR を変更するテストと直列化して、一時ディレクトリを指している最中に実行しない
    let _guard = super::env_lock();
    let result = mml_str_to_smf_bytes("cde");
    assert!(
        result.is_ok(),
        "mml_str_to_smf_bytes が失敗: {:?}",
        result.err()
    );
    let bytes = result.unwrap();
    // SMF は "MThd" で始まる
    assert!(bytes.starts_with(b"MThd"), "SMF が MThd で始まっていない");
    assert!(bytes.len() > 14, "SMF が短すぎる");
}

#[test]
fn mml_to_smf_bytes_strips_json_prefix() {
    // JSON プレフィックス付きの MML でも SMF が生成される
    // CMRT_BASE_DIR を変更するテストと直列化して、一時ディレクトリを指している最中に実行しない
    let _guard = super::env_lock();
    let mml = r#"{"Surge XT patch": "Pads/Pad 1.fxp"} cde"#;
    let result = mml_to_smf_bytes(mml);
    assert!(
        result.is_ok(),
        "mml_to_smf_bytes が失敗: {:?}",
        result.err()
    );
    let bytes = result.unwrap();
    assert!(bytes.starts_with(b"MThd"));
}

#[test]
fn mml_str_to_smf_bytes_empty_mml_returns_valid_smf() {
    // 空のMMLでも有効なSMFが生成されることを確認
    // CMRT_BASE_DIR を変更するテストと直列化して、一時ディレクトリを指している最中に実行しない
    let _guard = super::env_lock();
    let result = mml_str_to_smf_bytes("");
    assert!(
        result.is_ok(),
        "空のMMLでmml_str_to_smf_bytesが失敗: {:?}",
        result.err()
    );
    let bytes = result.unwrap();
    assert!(bytes.starts_with(b"MThd"));
}

#[test]
fn render_options_default_preroll_is_disabled() {
    let options = RenderOptions::default();

    assert_eq!(options.preroll(), RenderPreroll::Disabled);
    assert_eq!(options.preroll_samples(44_100.0), 0);
}

#[test]
fn render_options_preroll_ms_rounds_up_to_samples() {
    let options = RenderOptions::new().with_preroll_ms(1);

    assert_eq!(options.preroll(), RenderPreroll::from_millis(1));
    assert_eq!(options.preroll_samples(44_100.0), 45);
}

#[test]
fn apply_render_preroll_shifts_events_and_total_samples() {
    let events = vec![crate::midi::TimedMidiEvent {
        sample_pos: 12,
        message: crate::midi::MidiEvent::NoteOn {
            channel: 0,
            key: 60,
            velocity: 100,
        },
    }];

    let (events, total_samples) = apply_render_preroll(events, 34, 100);

    assert_eq!(events[0].sample_pos, 112);
    assert_eq!(total_samples, 134);
}

#[test]
fn trim_render_preroll_drops_leading_stereo_samples() {
    let trimmed = trim_render_preroll(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 1);

    assert_eq!(trimmed, vec![3.0, 4.0, 5.0, 6.0]);
}

#[test]
fn prepare_render_inputs_applies_configured_preroll() {
    let _guard = super::env_lock();
    let smf_bytes = mml_str_to_smf_bytes("t120o4c").unwrap();
    let config = CoreConfig {
        output_midi: "out.mid".into(),
        output_wav: "out.wav".into(),
        sample_rate: 1_000.0,
        buffer_size: 512,
        patch_path: None,
        patches_dir: None,
        random_patch: false,
    };

    let prepared = prepare_render_inputs(
        &smf_bytes,
        config,
        RenderOptions::new().with_preroll(RenderPreroll::from_millis(100)),
    )
    .unwrap();

    assert_eq!(prepared.preroll_samples, 100);
    assert_eq!(prepared.events[0].sample_pos, 100);
    assert!(
        prepared.total_samples > 100,
        "render length should include the preroll"
    );
}

#[test]
fn ensure_cmrt_dir_creates_directory_and_returns_path() {
    // 一時ディレクトリを使ってシステム設定ディレクトリを汚染しない
    let tmp = std::env::temp_dir().join("cmrt_test_ensure_cmrt_dir");
    let guard = super::EnvVarGuard::set("CMRT_BASE_DIR", &tmp);
    std::fs::remove_dir_all(&tmp).ok(); // 前回のテスト残骸を除去（存在しない場合は無視）

    let result = ensure_cmrt_dir();

    assert!(result.is_ok(), "ensure_cmrt_dir が失敗: {:?}", result.err());
    let dir = result.unwrap();
    assert!(
        dir.exists(),
        "clap-mml-render-tui/ ディレクトリが存在しない: {}",
        dir.display()
    );
    let dir_str = dir.to_string_lossy();
    assert!(
        dir_str.contains("clap-mml-render-tui"),
        "パスに clap-mml-render-tui が含まれていない: {}",
        dir_str
    );

    drop(guard); // CMRT_BASE_DIR を復元してからクリーンアップする
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn ensure_phrase_dir_creates_directory_and_returns_path() {
    let tmp = std::env::temp_dir().join("cmrt_test_ensure_phrase_dir");
    let guard = super::EnvVarGuard::set("CMRT_BASE_DIR", &tmp);
    std::fs::remove_dir_all(&tmp).ok();

    let result = ensure_phrase_dir();

    assert!(
        result.is_ok(),
        "ensure_phrase_dir が失敗: {:?}",
        result.err()
    );
    let dir = result.unwrap();
    assert!(
        dir.exists(),
        "phrase/ ディレクトリが存在しない: {}",
        dir.display()
    );
    assert!(
        dir.ends_with("phrase"),
        "パスが phrase で終わっていない: {}",
        dir.display()
    );
    assert!(
        dir.to_string_lossy().contains("clap-mml-render-tui"),
        "パスに clap-mml-render-tui が含まれていない: {}",
        dir.display()
    );

    drop(guard); // CMRT_BASE_DIR を復元してからクリーンアップする
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn ensure_daw_dir_creates_directory_and_returns_path() {
    let tmp = std::env::temp_dir().join("cmrt_test_ensure_daw_dir");
    let guard = super::EnvVarGuard::set("CMRT_BASE_DIR", &tmp);
    std::fs::remove_dir_all(&tmp).ok();

    let result = ensure_daw_dir();

    assert!(result.is_ok(), "ensure_daw_dir が失敗: {:?}", result.err());
    let dir = result.unwrap();
    assert!(
        dir.exists(),
        "daw/ ディレクトリが存在しない: {}",
        dir.display()
    );
    assert!(
        dir.ends_with("daw"),
        "パスが daw で終わっていない: {}",
        dir.display()
    );
    assert!(
        dir.to_string_lossy().contains("clap-mml-render-tui"),
        "パスに clap-mml-render-tui が含まれていない: {}",
        dir.display()
    );

    drop(guard); // CMRT_BASE_DIR を復元してからクリーンアップする
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn phrase_dir_and_daw_dir_are_siblings_under_cmrt() {
    // phrase/ と daw/ が同じ clap-mml-render-tui/ の下のサブディレクトリであることを確認する
    let tmp = std::env::temp_dir().join("cmrt_test_siblings");
    let guard = super::EnvVarGuard::set("CMRT_BASE_DIR", &tmp);
    std::fs::remove_dir_all(&tmp).ok();

    let phrase_dir = ensure_phrase_dir().unwrap();
    let daw_dir = ensure_daw_dir().unwrap();

    // 両方の親ディレクトリが同じであることを確認
    let phrase_parent = phrase_dir.parent().unwrap();
    let daw_parent = daw_dir.parent().unwrap();
    assert_eq!(
        phrase_parent,
        daw_parent,
        "phrase/ と daw/ が同じ親ディレクトリの下にない: {} vs {}",
        phrase_parent.display(),
        daw_parent.display()
    );

    drop(guard); // CMRT_BASE_DIR を復元してからクリーンアップする
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn env_var_guard_restores_previous_value_on_drop() {
    let key = "CMRT_TEST_ENV_GUARD_RESTORE";
    let original = std::env::var_os(key);
    std::env::set_var(key, "/tmp/cmrt_before_guard");

    {
        let guard = super::EnvVarGuard::set(key, "/tmp/cmrt_inside_guard");
        assert_eq!(std::env::var(key).as_deref(), Ok("/tmp/cmrt_inside_guard"));
        drop(guard);
    }

    assert_eq!(std::env::var(key).as_deref(), Ok("/tmp/cmrt_before_guard"));

    match original {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[cfg(unix)]
#[test]
fn env_var_guard_restores_previous_non_utf8_value_on_drop() {
    use std::os::unix::ffi::OsStringExt;

    let key = "CMRT_TEST_ENV_GUARD_RESTORE_NON_UTF8";
    let original = std::env::var_os(key);
    let before = std::ffi::OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xFF]);
    std::env::set_var(key, &before);

    {
        let guard = super::EnvVarGuard::set(key, "/tmp/cmrt_inside_guard");
        assert_eq!(std::env::var(key).as_deref(), Ok("/tmp/cmrt_inside_guard"));
        drop(guard);
    }

    assert_eq!(std::env::var_os(key), Some(before));

    match original {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[cfg(unix)]
#[test]
fn ensure_cmrt_dir_uses_non_utf8_env_override() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let _guard = super::env_lock();
    let original = std::env::var_os("CMRT_BASE_DIR");
    let mut base_bytes = std::env::temp_dir().into_os_string().into_vec();
    base_bytes.extend_from_slice(b"/cmrt_test_non_utf8_");
    base_bytes.push(0xFF);
    let base = std::ffi::OsString::from_vec(base_bytes);
    std::env::set_var("CMRT_BASE_DIR", &base);

    let result = ensure_cmrt_dir();

    match original {
        Some(value) => std::env::set_var("CMRT_BASE_DIR", value),
        None => std::env::remove_var("CMRT_BASE_DIR"),
    }

    assert!(result.is_ok(), "ensure_cmrt_dir が失敗: {:?}", result.err());
    let dir = result.unwrap();
    let dir_bytes = dir.as_os_str().as_bytes();
    assert!(
        dir_bytes
            .windows(base.as_bytes().len())
            .any(|window| window == base.as_bytes()),
        "非UTF-8の CMRT_BASE_DIR が反映されていない: {:?}",
        dir
    );

    std::fs::remove_dir_all(&dir).ok();
    if let Some(parent) = dir.parent() {
        std::fs::remove_dir(parent).ok();
    }
}
