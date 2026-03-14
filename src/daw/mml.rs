//! DawApp の MML 構築・拍子/テンポ解析メソッド

use super::{DawApp, FIRST_PLAYABLE_TRACK, MEASURES, TRACKS};
use super::timing::{compute_measure_samples, parse_beat_numerator, parse_tempo_bpm};

impl DawApp {
    // ─── MML 構築 ─────────────────────────────────────────────

    /// セル (track, measure) のレンダリング用 MML を構築する
    /// = track[t][0] (音色) + track0 全体 + track[t][m] (音符)
    /// 音色 JSON を先頭に置くことで extract_embedded_json が正しく解析できる
    pub(super) fn build_cell_mml(&self, track: usize, measure: usize) -> String {
        let track0: String = (0..=MEASURES)
            .map(|m| self.data[0][m].trim())
            .collect::<Vec<_>>()
            .join("");
        let timbre = self.data[track][0].trim();
        let notes = self.data[track][measure].trim();
        format!("{}{}{}", timbre, track0, notes)
    }

    /// 指定小節の全 track を結合した MML を構築する（1小節分の演奏用）
    /// track 0 はグローバルヘッダ（テンポ等）として各 track の先頭に付加するが、
    /// それ自体を独立した再生 track としては扱わない。
    /// 音色 JSON を先頭に置くことで extract_embedded_json が正しく解析できる
    pub(super) fn build_measure_mml(&self, measure: usize) -> String {
        let track0: String = (0..=MEASURES)
            .map(|m| self.data[0][m].trim())
            .collect::<Vec<_>>()
            .join("");

        let track_mmls: Vec<String> = (FIRST_PLAYABLE_TRACK..TRACKS)
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
        (1..=MEASURES)
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
        let track0: String = (0..=MEASURES)
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
