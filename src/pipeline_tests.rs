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
    assert!(content.starts_with(b"RIFF"), "WAV ファイルが RIFF ヘッダで始まっていない");
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
    // 中間ファイル（pass1_tokens.json 等）が CWD に書き出されるが、
    // 戻り値の計算自体はメモリ上で行われるため機能テストとして有効
    let result = mml_str_to_smf_bytes("cde");
    assert!(result.is_ok(), "mml_str_to_smf_bytes が失敗: {:?}", result.err());
    let bytes = result.unwrap();
    // SMF は "MThd" で始まる
    assert!(bytes.starts_with(b"MThd"), "SMF が MThd で始まっていない");
    assert!(bytes.len() > 14, "SMF が短すぎる");
}

#[test]
fn mml_to_smf_bytes_strips_json_prefix() {
    // JSON プレフィックス付きの MML でも SMF が生成される
    let mml = r#"{"Surge XT patch": "Pads/Pad 1.fxp"} cde"#;
    let result = mml_to_smf_bytes(mml);
    assert!(result.is_ok(), "mml_to_smf_bytes が失敗: {:?}", result.err());
    let bytes = result.unwrap();
    assert!(bytes.starts_with(b"MThd"));
}

#[test]
fn mml_str_to_smf_bytes_empty_mml_returns_valid_smf() {
    // 空のMMLでも有効なSMFが生成されることを確認
    let result = mml_str_to_smf_bytes("");
    assert!(result.is_ok(), "空のMMLでmml_str_to_smf_bytesが失敗: {:?}", result.err());
    let bytes = result.unwrap();
    assert!(bytes.starts_with(b"MThd"));
}
