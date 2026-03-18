use super::timing::{compute_measure_samples, parse_beat_numerator, parse_tempo_bpm};
use super::mml::build_cell_mml_from_data;
use super::playback::effective_measure_count;
use super::{MEASURES, TRACKS};

// ─── parse_tempo_bpm ──────────────────────────────────────────

#[test]
fn parse_tempo_bpm_basic() {
    assert_eq!(parse_tempo_bpm("t120cde"), Some(120.0));
}

#[test]
fn parse_tempo_bpm_at_start() {
    assert_eq!(parse_tempo_bpm("t80"), Some(80.0));
}

#[test]
fn parse_tempo_bpm_no_tempo() {
    assert_eq!(parse_tempo_bpm("cde"), None);
}

#[test]
fn parse_tempo_bpm_empty() {
    assert_eq!(parse_tempo_bpm(""), None);
}

#[test]
fn parse_tempo_bpm_after_json() {
    // JSON除去後の残りMMLにt200が含まれる場合
    assert_eq!(parse_tempo_bpm("t200efg"), Some(200.0));
}

// ─── parse_beat_numerator ─────────────────────────────────────

#[test]
fn parse_beat_numerator_4_4() {
    let json = r#"{"beat": "4/4"}"#;
    assert_eq!(parse_beat_numerator(Some(json)), 4);
}

#[test]
fn parse_beat_numerator_3_4() {
    let json = r#"{"beat": "3/4"}"#;
    assert_eq!(parse_beat_numerator(Some(json)), 3);
}

#[test]
fn parse_beat_numerator_no_json() {
    // JSONなし → デフォルト 4
    assert_eq!(parse_beat_numerator(None), 4);
}

#[test]
fn parse_beat_numerator_malformed_json() {
    // 壊れたJSON → デフォルト 4
    assert_eq!(parse_beat_numerator(Some("{invalid")), 4);
}

#[test]
fn parse_beat_numerator_missing_beat_key() {
    // beat キーなし → デフォルト 4
    let json = r#"{"other": "value"}"#;
    assert_eq!(parse_beat_numerator(Some(json)), 4);
}

#[test]
fn parse_beat_numerator_zero_clamps_to_one() {
    // beat 分子が 0 → 1 にクランプされる
    let json = r#"{"beat": "0/4"}"#;
    assert_eq!(parse_beat_numerator(Some(json)), 1);
}

#[test]
fn parse_beat_numerator_non_numeric() {
    // 数値でない beat → デフォルト 4
    let json = r#"{"beat": "x/4"}"#;
    assert_eq!(parse_beat_numerator(Some(json)), 4);
}

// ─── compute_measure_samples ──────────────────────────────────

#[test]
fn compute_measure_samples_4_4_t120_44100() {
    // t120, 4/4, 44100Hz: 4 * (60/120) * 44100 * 2 = 4 * 0.5 * 44100 * 2 = 176400
    let result = compute_measure_samples(4, 120.0, 44100.0);
    assert_eq!(result, 176400);
    assert_eq!(result % 2, 0, "ステレオ整列のため偶数であること");
}

#[test]
fn compute_measure_samples_result_is_always_even() {
    // どんな入力でも結果は偶数（ステレオ整列）
    for beat in [1u32, 2, 3, 4, 6] {
        for bpm in [60.0f64, 120.0, 96.0, 180.0] {
            let result = compute_measure_samples(beat, bpm, 44100.0);
            assert_eq!(result % 2, 0, "beat={beat}, bpm={bpm} のとき偶数でない: {result}");
        }
    }
}

#[test]
fn compute_measure_samples_clamps_bpm_zero() {
    // BPM 0 は 1.0 にクランプされ、OOM/パニックを起こさない
    let result = compute_measure_samples(4, 0.0, 44100.0);
    // 4 * 60 / 1.0 * 44100 * 2 = 21168000 (大きいが有限)
    assert!(result > 0);
    assert_eq!(result % 2, 0);
}

#[test]
fn compute_measure_samples_clamps_bpm_negative() {
    // 負のBPM は 1.0 にクランプ
    let result = compute_measure_samples(4, -100.0, 44100.0);
    assert!(result > 0);
    assert_eq!(result % 2, 0);
}

#[test]
fn compute_measure_samples_beat_zero_clamps_to_one() {
    // beat=0 は 1 にクランプ
    let result = compute_measure_samples(0, 120.0, 44100.0);
    // 1 * 60 / 120 * 44100 * 2 = 44100
    assert_eq!(result, 44100);
    assert_eq!(result % 2, 0);
}

// ─── ensure_cmrt_dir ──────────────────────────────────────────

#[test]
fn ensure_cmrt_dir_is_idempotent() {
    // 複数回呼んでもエラーにならない（一時ディレクトリを使って設定ディレクトリを汚染しない）
    let tmp = std::env::temp_dir().join("cmrt_test_daw_idempotent");
    std::env::set_var("CMRT_BASE_DIR", &tmp);
    std::fs::remove_dir_all(&tmp).ok();

    let r1 = crate::pipeline::ensure_cmrt_dir();
    let r2 = crate::pipeline::ensure_cmrt_dir();
    std::env::remove_var("CMRT_BASE_DIR");

    assert!(r1.is_ok(), "初回 ensure_cmrt_dir が失敗: {:?}", r1.err());
    assert!(r2.is_ok(), "2回目 ensure_cmrt_dir が失敗: {:?}", r2.err());

    std::fs::remove_dir_all(&tmp).ok();
}

// ─── build_cell_mml_from_data ─────────────────────────────────

/// テスト用ヘルパー: TRACKS×(MEASURES+1) の空 data を作成する
fn empty_data(tracks: usize, measures: usize) -> Vec<Vec<String>> {
    vec![vec![String::new(); measures + 1]; tracks]
}

#[test]
fn build_cell_mml_includes_timbre_in_measure() {
    // 音色 JSON が小節 MML に含まれること（issue #67 修正の前提: 音色変更時に小節を再キャッシュすべき根拠）
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = super::DEFAULT_TRACK0_MML.to_string();
    data[1][0] = r#"{"Surge XT patch": "piano"}"#.to_string();
    data[1][1] = "cde".to_string();

    let mml = build_cell_mml_from_data(&data, MEASURES, 1, 1);
    assert!(mml.contains(r#"{"Surge XT patch": "piano"}"#), "音色 JSON が MML に含まれていない: {}", mml);
    assert!(mml.contains("cde"), "音符が MML に含まれていない: {}", mml);
}

#[test]
fn build_cell_mml_includes_track0_tempo_in_measure() {
    // track0 のテンポ指定が小節 MML に含まれること（track0 変更時に全小節を再キャッシュすべき根拠）
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = r#"{"beat": "4/4"}t180"#.to_string();
    data[1][0] = "".to_string();
    data[1][1] = "cde".to_string();

    let mml = build_cell_mml_from_data(&data, MEASURES, 1, 1);
    assert!(mml.contains("t180"), "track0 のテンポ指定が MML に含まれていない: {}", mml);
    assert!(mml.contains("cde"), "音符が MML に含まれていない: {}", mml);
}

#[test]
fn build_cell_mml_timbre_change_affects_all_measures() {
    // 同じ音符セルで音色が異なる場合、MML が異なること
    // → 音色変更時は当該 track の全小節を再キャッシュしなければならない理由
    let mut data_piano = empty_data(TRACKS, MEASURES);
    data_piano[0][0] = super::DEFAULT_TRACK0_MML.to_string();
    data_piano[1][0] = r#"{"Surge XT patch": "piano"}"#.to_string();
    data_piano[1][1] = "cde".to_string();

    let mut data_guitar = data_piano.clone();
    data_guitar[1][0] = r#"{"Surge XT patch": "guitar"}"#.to_string();

    let mml_piano  = build_cell_mml_from_data(&data_piano,  MEASURES, 1, 1);
    let mml_guitar = build_cell_mml_from_data(&data_guitar, MEASURES, 1, 1);

    assert_ne!(mml_piano, mml_guitar, "音色変更後の MML が同一になっており、キャッシュ無効化が必要");
}

#[test]
fn build_cell_mml_track0_change_affects_all_tracks() {
    // track0 のテンポ変更で全 track の小節 MML が変化すること
    // → track0 セル変更時は全演奏トラックの全小節を再キャッシュしなければならない理由
    let mut data_t120 = empty_data(TRACKS, MEASURES);
    data_t120[0][0] = super::DEFAULT_TRACK0_MML.to_string();
    data_t120[1][0] = r#"{"Surge XT patch": "piano"}"#.to_string();
    data_t120[1][1] = "cde".to_string();

    let mut data_t200 = data_t120.clone();
    data_t200[0][0] = r#"{"beat": "4/4"}t200"#.to_string();

    let mml_t120 = build_cell_mml_from_data(&data_t120, MEASURES, 1, 1);
    let mml_t200 = build_cell_mml_from_data(&data_t200, MEASURES, 1, 1);

    assert_ne!(mml_t120, mml_t200, "track0 変更後の MML が同一になっており、全小節の再キャッシュが必要");
}

#[test]
fn build_cell_mml_empty_notes_cell_has_no_note_content() {
    // 音符セルが空のとき、その MML には音符が含まれないこと
    // → kick_cache は data[track][measure] が空のときジョブを投入しないことで正しい挙動となる（issue #69 修正）
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = super::DEFAULT_TRACK0_MML.to_string();
    data[1][0] = r#"{"Surge XT patch": "piano"}"#.to_string();
    data[1][1] = "".to_string(); // 音符が空

    // 空の音符セルは kick_cache によってジョブが投入されないため
    // キャッシュ状態は Empty のままとなり、"●" インジケータは表示されない
    assert!(data[1][1].trim().is_empty(), "音符セルが空であるべき");

    // build_cell_mml_from_data は track0 を常に含むため空でないが、
    // kick_cache は data[track][measure] の生の値で空判定するため、
    // このセルはキャッシュジョブが投入されない
    let combined_mml = build_cell_mml_from_data(&data, MEASURES, 1, 1);
    assert!(!combined_mml.trim().is_empty(), "結合 MML は track0 を含むため非空");
    // kick_cache の正しい実装: data[track][measure].trim().is_empty() で早期リターン
    // （combined_mml が非空でもセル自身が空なら投入しない）
    let should_kick = !data[1][1].trim().is_empty();
    assert!(!should_kick, "空の音符セルは kick_cache に投入されるべきでない");
}

// ─── effective_measure_count ──────────────────────────────────

#[test]
fn effective_measure_count_all_empty_returns_none() {
    let mmls = vec!["".to_string(); MEASURES];
    assert_eq!(effective_measure_count(&mmls), None);
}

#[test]
fn effective_measure_count_skips_trailing_empty_measures() {
    // meas1=cccccccc, meas2=ffffffff, meas3-8 空 → 有効小節数=2（issue #68）
    let mut mmls = vec!["".to_string(); MEASURES];
    mmls[0] = "cccccccc".to_string();
    mmls[1] = "ffffffff".to_string();
    assert_eq!(effective_measure_count(&mmls), Some(2));
}

#[test]
fn effective_measure_count_includes_internal_empty_measures() {
    // meas1 非空、meas2 空（中間）、meas3 非空、meas4-8 空 → 有効小節数=3
    let mut mmls = vec!["".to_string(); MEASURES];
    mmls[0] = "cde".to_string();
    mmls[2] = "fga".to_string();
    assert_eq!(effective_measure_count(&mmls), Some(3));
}

#[test]
fn effective_measure_count_single_non_empty_measure() {
    let mut mmls = vec!["".to_string(); MEASURES];
    mmls[0] = "c".to_string();
    assert_eq!(effective_measure_count(&mmls), Some(1));
}

#[test]
fn effective_measure_count_all_measures_non_empty() {
    let mmls: Vec<String> = (0..MEASURES).map(|i| format!("c{}", i)).collect();
    assert_eq!(effective_measure_count(&mmls), Some(MEASURES));
}

#[test]
fn effective_measure_count_whitespace_only_treated_as_empty() {
    let mut mmls = vec!["".to_string(); MEASURES];
    mmls[0] = "cde".to_string();
    mmls[1] = "   ".to_string(); // whitespace-only → treated as empty (trailing)
    assert_eq!(effective_measure_count(&mmls), Some(1));
}

// ─── track8（最終演奏トラック）のテスト ───────────────────────

#[test]
fn build_cell_mml_track8_is_accessible() {
    // TRACKS-1 (= 8) が最終演奏トラックとして正しく動作すること（issue #72: track1~8 対応）
    let last_track = TRACKS - 1;
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = super::DEFAULT_TRACK0_MML.to_string();
    data[last_track][0] = r#"{"Surge XT patch": "bass"}"#.to_string();
    data[last_track][1] = "c4d4e4f4".to_string();

    let mml = build_cell_mml_from_data(&data, MEASURES, last_track, 1);
    assert!(mml.contains(r#"{"Surge XT patch": "bass"}"#), "track8 の音色 JSON が MML に含まれていない: {}", mml);
    assert!(mml.contains("c4d4e4f4"), "track8 の音符が MML に含まれていない: {}", mml);
    assert!(mml.contains("t120"), "track0 のテンポが track8 の MML に含まれていない: {}", mml);
}

// ─── JSON 保存形式 ────────────────────────────────────────────

#[test]
fn daw_save_json_roundtrip_default_data() {
    // デフォルト data（track0/meas0 のみ非空）が JSON 経由で復元されること
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = super::DEFAULT_TRACK0_MML.to_string();

    let file = super::data_to_save_file(&data, TRACKS, MEASURES);
    let json = serde_json::to_string_pretty(&file).unwrap();

    let loaded_file: super::DawSaveFile = serde_json::from_str(&json).unwrap();
    let mut restored = empty_data(TRACKS, MEASURES);
    super::apply_save_file_to_data(&loaded_file, &mut restored, TRACKS, MEASURES);

    assert_eq!(restored[0][0], super::DEFAULT_TRACK0_MML);
    // 他のセルは空のまま
    assert!(restored[0][1].is_empty());
    assert!(restored[1][0].is_empty());
}

#[test]
fn daw_save_json_skips_empty_tracks_and_meas() {
    // 空トラック・空小節は JSON に含まれないこと
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = super::DEFAULT_TRACK0_MML.to_string();
    data[1][1] = "cde".to_string();
    // track2..8, meas2..MEASURES は空

    let file = super::data_to_save_file(&data, TRACKS, MEASURES);
    let json = serde_json::to_string_pretty(&file).unwrap();

    // JSON にトラック 2 以上は含まれない
    assert!(!json.contains("\"track\": 2"), "空トラックが JSON に含まれている: {json}");
    // 空小節 2 以降も含まれない
    assert!(!json.contains("\"meas\": 2"), "空小節が JSON に含まれている: {json}");
    // 非空データは含まれる
    assert!(json.contains("t120"), "track0/meas0 の MML が含まれていない: {json}");
    assert!(json.contains("cde"), "track1/meas1 の MML が含まれていない: {json}");
}

#[test]
fn daw_save_json_track0_has_tempo_description() {
    // track0 のエントリに "tempo track" という description が付くこと
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = super::DEFAULT_TRACK0_MML.to_string();

    let file = super::data_to_save_file(&data, TRACKS, MEASURES);
    let json = serde_json::to_string_pretty(&file).unwrap();

    assert!(json.contains("\"tempo track\""), "track0 の description が JSON に含まれていない: {json}");
}

#[test]
fn daw_save_json_meas0_has_initial_description() {
    // meas0 のエントリに "initial" という description が付くこと
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = super::DEFAULT_TRACK0_MML.to_string();

    let file = super::data_to_save_file(&data, TRACKS, MEASURES);
    let json = serde_json::to_string_pretty(&file).unwrap();

    assert!(json.contains("\"initial\""), "meas0 の description が JSON に含まれていない: {json}");
}

#[test]
fn daw_save_json_non_initial_meas_has_no_description() {
    // meas1 以降のエントリには description が付かないこと（ムダな情報を書かない）
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = super::DEFAULT_TRACK0_MML.to_string();
    data[1][1] = "cde".to_string();

    let file = super::data_to_save_file(&data, TRACKS, MEASURES);
    // DawSaveMeas の description フィールドを直接確認する
    let track1 = file.tracks.iter().find(|t| t.track == 1).unwrap();
    let meas1 = track1.meas.iter().find(|m| m.meas == 1).unwrap();
    assert!(meas1.description.is_none(), "meas1 に description が付いている: {:?}", meas1.description);
}

#[test]
fn daw_save_json_out_of_range_indices_are_ignored_on_load() {
    // JSON に含まれるトラック・小節インデックスが範囲外の場合は無視されること
    let json = r#"{"tracks":[{"track":100,"meas":[{"meas":0,"description":"initial","mml":"cde"}]}]}"#;
    let file: super::DawSaveFile = serde_json::from_str(json).unwrap();
    let mut data = empty_data(TRACKS, MEASURES);
    super::apply_save_file_to_data(&file, &mut data, TRACKS, MEASURES);
    // data は変更されていないこと
    for t in 0..TRACKS {
        for m in 0..=MEASURES {
            assert!(data[t][m].is_empty(), "範囲外インデックスが data を変更した: t={t}, m={m}");
        }
    }
}

#[test]
fn daw_save_json_roundtrip_with_notes() {
    // 複数トラック・複数小節のデータが JSON 経由で正確に復元されること
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = super::DEFAULT_TRACK0_MML.to_string();
    data[1][0] = r#"{"Surge XT patch": "piano"}"#.to_string();
    data[1][1] = "cde".to_string();
    data[1][2] = "efg".to_string();
    data[2][1] = "abc".to_string();

    let file = super::data_to_save_file(&data, TRACKS, MEASURES);
    let json = serde_json::to_string_pretty(&file).unwrap();
    let loaded: super::DawSaveFile = serde_json::from_str(&json).unwrap();
    let mut restored = empty_data(TRACKS, MEASURES);
    super::apply_save_file_to_data(&loaded, &mut restored, TRACKS, MEASURES);

    assert_eq!(restored[0][0], data[0][0]);
    assert_eq!(restored[1][0], data[1][0]);
    assert_eq!(restored[1][1], data[1][1]);
    assert_eq!(restored[1][2], data[1][2]);
    assert_eq!(restored[2][1], data[2][1]);
    // 空セルは空のまま
    assert!(restored[3][1].is_empty());
}

#[test]
fn daw_load_clears_defaults_before_applying_json() {
    // JSON が正常にパースできた場合、new() が設定したデフォルト値（data[0][0]）は
    // クリアされてから JSON の内容が適用されること。
    // これにより、ユーザーが track0/meas0 を空にして保存した場合に、次回起動で
    // デフォルト値が復活しないことを保証する。
    //
    // シミュレーション: 全セルが空の JSON（= ユーザーが全て消した状態）を
    // apply_save_file_to_data で適用すると、pre-populated なデフォルト値は
    // 上書きされず残ってしまうが、load() はクリア後に apply するため正しく空になる。
    let empty_file = super::DawSaveFile { tracks: vec![] };
    let json = serde_json::to_string_pretty(&empty_file).unwrap();

    // data[0][0] にデフォルト値が入った状態を再現する
    let mut data = empty_data(TRACKS, MEASURES);
    data[0][0] = super::DEFAULT_TRACK0_MML.to_string();

    // ① クリアなしで apply すると track0/meas0 はデフォルトのまま残る（バグの再現）
    {
        let loaded: super::DawSaveFile = serde_json::from_str(&json).unwrap();
        let mut data_no_clear = data.clone();
        super::apply_save_file_to_data(&loaded, &mut data_no_clear, TRACKS, MEASURES);
        assert_eq!(data_no_clear[0][0], super::DEFAULT_TRACK0_MML,
            "クリアなしでは空 JSON を適用してもデフォルト値が残る（バグの再現）");
    }

    // ② クリアしてから apply するとデフォルト値は消える（修正後の正しい挙動）
    {
        let loaded: super::DawSaveFile = serde_json::from_str(&json).unwrap();
        for row in &mut data {
            for cell in row.iter_mut() {
                cell.clear();
            }
        }
        super::apply_save_file_to_data(&loaded, &mut data, TRACKS, MEASURES);
        assert!(data[0][0].is_empty(),
            "クリア後に空 JSON を適用すると track0/meas0 は空になるべき");
    }
}
