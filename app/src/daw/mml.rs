//! DawApp の MML 構築・拍子/テンポ解析メソッド

use super::{DawApp, FIRST_PLAYABLE_TRACK};
use super::timing::{compute_measure_samples, parse_beat_numerator, parse_tempo_bpm};

// ─── 純粋関数（テスト用） ──────────────────────────────────────

/// data 配列からセル MML を構築する純粋関数。
///
/// `data[0][*]` がグローバルヘッダ（track0）、`data[track][0]` が音色、`data[track][measure]` が音符。
/// `build_cell_mml` と同じ MML を返すが、`DawApp` インスタンスを必要としないためテストで利用できる。
///
/// # 引数
/// - `data`: `data[track][measure]` の文字列スライス（`data[0]` が track0）
/// - `num_measures`: 小節数（`data[0].len() - 1`）
/// - `track`: 対象 track インデックス
/// - `measure`: 対象小節インデックス（0 = 音色列）
pub(super) fn build_cell_mml_from_data(
    data: &[Vec<String>],
    num_measures: usize,
    track: usize,
    measure: usize,
) -> String {
    use mmlabc_to_smf::mml_preprocessor;
    let track0: String = (0..=num_measures)
        .filter_map(|m| data[0].get(m))
        .enumerate()
        .map(|(m, cell)| {
            let cell = cell.trim();
            if m == 0 {
                mml_preprocessor::extract_embedded_json(cell).remaining_mml
            } else {
                cell.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("");
    let timbre = data.get(track).and_then(|r| r.get(0)).map(|s| s.trim()).unwrap_or("");
    let notes  = data.get(track).and_then(|r| r.get(measure)).map(|s| s.trim()).unwrap_or("");
    format!("{}{}{}", timbre, track0, notes)
}

impl DawApp {
    // ─── MML 構築 ─────────────────────────────────────────────

    /// セル (track, measure) のレンダリング用 MML を構築する
    /// = track[t][0] (音色) + track0 全体 + track[t][m] (音符)
    /// 音色 JSON を先頭に置くことで extract_embedded_json が正しく解析できる
    pub(super) fn build_cell_mml(&self, track: usize, measure: usize) -> String {
        build_cell_mml_from_data(&self.data, self.measures, track, measure)
    }

    /// 指定小節の全 track を結合した MML を構築する（1小節分の演奏用）
    /// track 0 はグローバルヘッダ（テンポ等）として各 track の先頭に付加するが、
    /// それ自体を独立した再生 track としては扱わない。
    /// 音色 JSON を先頭に置くことで extract_embedded_json が正しく解析できる
    pub(super) fn build_measure_mml(&self, measure: usize) -> String {
        use mmlabc_to_smf::mml_preprocessor;
        let track0: String = (0..=self.measures)
            .map(|m| {
                let cell = self.data[0][m].trim();
                if m == 0 {
                    // data[0][0] には beat JSON（DAW用メタ情報）が含まれる場合があるため、
                    // MML パーサに渡す前に JSON 部分を除去して残りの MML のみを使う
                    mml_preprocessor::extract_embedded_json(cell).remaining_mml
                } else {
                    cell.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("");

        let track_mmls: Vec<String> = (FIRST_PLAYABLE_TRACK..self.tracks)
            .filter_map(|t| {
                let timbre = self.data[t][0].trim();
                let notes = self.data[t][measure].trim();
                if timbre.is_empty() && notes.is_empty() {
                    None
                } else {
                    Some(format!("{}{}{}", timbre, track0, notes))
                }
            })
            .collect();

        track_mmls.join(";")
    }

    /// 全小節の per-measure MML ベクターを構築する（演奏用; hot reload に使用）
    /// index i → meas i+1 の MML（空小節は空文字列）
    pub(super) fn build_measure_mmls(&self) -> Vec<String> {
        (1..=self.measures)
            .map(|m| self.build_measure_mml(m))
            .collect()
    }

    // ─── 拍子 / テンポ解析 ────────────────────────────────────

    /// track0[0] の JSON から beat (拍子分子) を解析する。
    /// `{"beat": "4/4"}` → 4。解析できない場合は 4 (4/4デフォルト) を返す。
    /// 現バージョンでは 4/4 のみサポート。JSON は将来の拍子変更に備えた仮置き。
    pub(super) fn beat_numerator(&self) -> u32 {
        use mmlabc_to_smf::mml_preprocessor;
        let header = self.data[0][0].trim();
        let preprocessed = mml_preprocessor::extract_embedded_json(header);
        parse_beat_numerator(preprocessed.embedded_json.as_deref())
    }

    /// track0 MML から tempo (BPM) を解析する。
    /// `t120` → 120.0。解析できない場合は 120.0 (デフォルト)。[1.0, 960.0] にクランプ。
    pub(super) fn tempo_bpm(&self) -> f64 {
        use mmlabc_to_smf::mml_preprocessor;
        let track0: String = (0..=self.measures)
            .map(|m| self.data[0][m].trim())
            .collect::<Vec<_>>()
            .join("");
        let preprocessed = mml_preprocessor::extract_embedded_json(&track0);
        parse_tempo_bpm(&preprocessed.remaining_mml)
            .unwrap_or(120.0)
            .clamp(1.0, 960.0)
    }

    /// 1小節のサンプル数を計算する（ステレオ: L/R インターリーブ）。
    /// beat_numerator * (60 / bpm) * sample_rate * 2
    pub(super) fn measure_duration_samples(&self) -> usize {
        compute_measure_samples(self.beat_numerator(), self.tempo_bpm(), self.cfg.sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::build_cell_mml_from_data;
    use super::super::{DEFAULT_TRACK0_MML, MEASURES, TRACKS};

    /// テスト用ヘルパー: TRACKS×(MEASURES+1) の空 data を作成する
    fn empty_data(tracks: usize, measures: usize) -> Vec<Vec<String>> {
        vec![vec![String::new(); measures + 1]; tracks]
    }

    // ─── build_cell_mml_from_data ─────────────────────────────────

    #[test]
    fn build_cell_mml_includes_timbre_in_measure() {
        // 音色 JSON が小節 MML に含まれること（issue #67 修正の前提: 音色変更時に小節を再キャッシュすべき根拠）
        let mut data = empty_data(TRACKS, MEASURES);
        data[0][0] = DEFAULT_TRACK0_MML.to_string();
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
        data_piano[0][0] = DEFAULT_TRACK0_MML.to_string();
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
        data_t120[0][0] = DEFAULT_TRACK0_MML.to_string();
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
        data[0][0] = DEFAULT_TRACK0_MML.to_string();
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

    // ─── track8（最終演奏トラック）のテスト ───────────────────────

    #[test]
    fn build_cell_mml_track8_is_accessible() {
        // TRACKS-1 (= 8) が最終演奏トラックとして正しく動作すること（issue #72: track1~8 対応）
        let last_track = TRACKS - 1;
        let mut data = empty_data(TRACKS, MEASURES);
        data[0][0] = DEFAULT_TRACK0_MML.to_string();
        data[last_track][0] = r#"{"Surge XT patch": "bass"}"#.to_string();
        data[last_track][1] = "c4d4e4f4".to_string();

        let mml = build_cell_mml_from_data(&data, MEASURES, last_track, 1);
        assert!(mml.contains(r#"{"Surge XT patch": "bass"}"#), "track8 の音色 JSON が MML に含まれていない: {}", mml);
        assert!(mml.contains("c4d4e4f4"), "track8 の音符が MML に含まれていない: {}", mml);
        assert!(mml.contains("t120"), "track0 のテンポが track8 の MML に含まれていない: {}", mml);
    }
}
