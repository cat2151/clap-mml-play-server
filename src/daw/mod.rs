//! DAW 風モード
//!
//! 8 tracks × (0..=8 measures) の matrix
//!   measure 0 = 音色 (timbre) / track ごとの共通ヘッダ
//!   track   0 = 拍子JSON + テンポ (例: `{"beat": "4/4"}t120`) → render 時に全小節の先頭にくっつける
//!
//! キー操作 (NORMAL):
//!   h/l    : 小節 (列) 移動
//!   j/k    : track (行) 移動
//!   H      : 先頭 track へ移動
//!   M      : 中央 track へ移動
//!   L      : 末尾 track へ移動
//!   i      : INSERT モード（現在セルを編集）
//!   p      : 演奏 / 停止 toggle
//!   r      : measure 0 にランダム音色を設定
//!   K      : ヘルプ表示
//!   q      : アプリ終了
//!   d / ESC: DAW モード終了 → TUI に戻る
//!
//! キー操作 (INSERT):
//!   ESC   : 確定 → NORMAL
//!   Enter : 確定 → 次の小節へ移動 → INSERT 継続
//!   ;     : 確定時にセミコロンで分割し、下の track に順に追加
//!
//! キー操作 (HELP):
//!   ESC   : キャンセル → NORMAL

mod input;
mod ui;

use anyhow::Result;
use clack_host::prelude::PluginEntry;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{backend::CrosstermBackend, Frame, Terminal};
use tui_textarea::TextArea;

use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::patch_list::{collect_patches, to_relative};

/// track 数（固定）
pub const TRACKS: usize = 8;
/// 小節数（固定）。measure 0 = 音色列。measure 1..=MEASURES = 通常小節。
pub const MEASURES: usize = 8;
/// track 0 はグローバルヘッダ（テンポ等）専用。演奏 track は 1 から始まる。
const FIRST_PLAYABLE_TRACK: usize = 1;

const DAW_FILE: &str = "daw.txt";
const DAW_MML_DEBUG_FILE: &str = "cmrt/daw_mml_debug.txt";

/// MML 文字列から最初の `tNNN` パターンを探し、BPM を返す
pub(super) fn parse_tempo_bpm(mml: &str) -> Option<f64> {
    let mut chars = mml.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == 't' {
            let mut num_str = String::new();
            while chars.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                num_str.push(chars.next().unwrap());
            }
            if !num_str.is_empty() {
                return num_str.parse().ok();
            }
        }
    }
    None
}

/// JSON 文字列から beat 分子を解析する。
/// `{"beat": "4/4"}` → 4。解析できない場合は 4 (4/4デフォルト) を返す。最小値は 1。
pub(super) fn parse_beat_numerator(json_str: Option<&str>) -> u32 {
    json_str
        .and_then(|j| {
            let v: serde_json::Value = serde_json::from_str(j).ok()?;
            let s = v.get("beat")?.as_str()?;
            s.split('/').next()?.parse::<u32>().ok()
        })
        .unwrap_or(4)
        .max(1)
}

/// BPM と beat 分子からステレオサンプル数を計算する純粋関数。
/// BPM は [1.0, 960.0] にクランプする。結果はステレオ整列のため偶数にする。
pub(super) fn compute_measure_samples(beat: u32, bpm: f64, sample_rate: f64) -> usize {
    let bpm = bpm.clamp(1.0, 960.0);
    let beat = beat.max(1);
    let secs = (beat as f64 * 60.0) / bpm;
    let raw = (secs * sample_rate * 2.0).round() as usize;
    // ステレオ (L/R インターリーブ) 整列のため偶数に切り上げる
    raw + (raw & 1)
}

// ─── キャッシュ ───────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub(super) enum CacheState {
    Empty,   // MML が空
    Pending, // MML あり、レンダリング待ち or 実行中
    Ready,   // レンダリング済み
    Error,   // レンダリング失敗
}

#[derive(Clone)]
pub(super) struct CellCache {
    pub(super) state: CacheState,
}

impl CellCache {
    fn empty() -> Self {
        Self { state: CacheState::Empty }
    }
}

// ─── 演奏状態 ─────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub(super) enum DawPlayState {
    Idle,
    Playing,
}

// ─── 内部モード ───────────────────────────────────────────────

#[derive(PartialEq)]
pub(super) enum DawMode {
    Normal,
    Insert,
    Help,
}

/// handle_normal の戻り値
pub(super) enum DawNormalAction {
    Continue,
    ReturnToTui,
    QuitApp,
}

/// DAW モード終了後の TUI への通知
pub enum DawExitReason {
    /// d / ESC キーで TUI に戻る
    ReturnToTui,
    /// q キーまたは Ctrl+C でアプリを終了する
    QuitApp,
}

// ─── DawApp ───────────────────────────────────────────────────

pub struct DawApp {
    /// data[track][measure]: track 0..TRACKS, measure 0..=MEASURES
    pub(super) data: Vec<Vec<String>>,

    pub(super) cursor_track: usize,   // 0..TRACKS-1
    pub(super) cursor_measure: usize, // 0..=MEASURES  (0 = 音色列)

    pub(super) mode: DawMode,
    pub(super) textarea: TextArea<'static>,

    cfg: Arc<Config>,
    entry_ptr: usize, // *const PluginEntry as usize (main() に生存保証)

    /// セルごとのキャッシュ [track][measure]
    pub(super) cache: Arc<Mutex<Vec<Vec<CellCache>>>>,

    /// キャッシュワーカースレッドへのジョブチャネル: (track, measure, mml)
    /// シリアルな単一ワーカーで処理することでファイル書き込みの競合を防ぐ
    cache_tx: std::sync::mpsc::Sender<(usize, usize, String)>,

    /// `mml_render_for_cache` の排他実行ロック。
    /// キャッシュワーカーと再生スレッドが同時に daw_cache.mid/wav を書き込まないよう、
    /// `mml_render_for_cache` 呼び出し前に必ずこのロックを取得すること。
    render_lock: Arc<Mutex<()>>,

    pub(super) play_state: Arc<Mutex<DawPlayState>>,

    /// 再生スレッドと共有する各小節の MML ベクター (MEASURES 要素, index i → meas i+1)。
    /// セル編集・ランダム音色変更のたびに更新されることで、
    /// play 中でも次ループ冒頭から新しい MML が反映される（hot reload）。
    play_measure_mmls: Arc<Mutex<Vec<String>>>,

    /// 再生スレッドと共有する 1 小節のステレオサンプル数。
    /// セル編集・ランダム音色変更のたびに `play_measure_mmls` と一緒に更新される。
    play_measure_samples: Arc<Mutex<usize>>,
}

impl DawApp {
    pub fn new(cfg: Arc<Config>, entry_ptr: usize) -> Self {
        let mut data = vec![vec![String::new(); MEASURES + 1]; TRACKS];
        // track 0 のデフォルトは拍子指定 JSON + テンポ設定
        data[0][0] = r#"{"beat": "4/4"}t120"#.to_string();

        let cache = Arc::new(Mutex::new(
            vec![vec![CellCache::empty(); MEASURES + 1]; TRACKS],
        ));

        // シリアルなキャッシュワーカースレッドを起動する。
        // チャネルが送信側（cache_tx）を介してジョブを受け取り順次レンダリングすることで
        // ファイル書き込み（cmrt/pass1_tokens.json 等）の競合と過剰スレッド生成を防ぐ。
        let (cache_tx, cache_rx) = std::sync::mpsc::channel::<(usize, usize, String)>();

        // `mml_render_for_cache` はキャッシュワーカーと再生スレッドの両方から呼ばれるため、
        // cmrt/daw_cache.mid/wav への同時書き込みを防ぐ排他ロックを共有する。
        let render_lock: Arc<Mutex<()>> = Arc::new(Mutex::new(()));

        {
            let cache_worker = Arc::clone(&cache);
            let cfg_worker = Arc::clone(&cfg);
            let render_lock_worker = Arc::clone(&render_lock);
            std::thread::spawn(move || {
                // SAFETY: entry は main() のスタックに生存している
                let entry_ref: &PluginEntry = unsafe { &*(entry_ptr as *const PluginEntry) };
                let mut daw_cfg = (*cfg_worker).clone();
                daw_cfg.random_patch = false;

                for (track, measure, mml) in cache_rx {
                    let _guard = render_lock_worker.lock().unwrap();
                    match crate::pipeline::mml_render_for_cache(&mml, &daw_cfg, entry_ref) {
                        Ok(samples) => {
                            // 開発用: track/measure ごとに WAV ファイルを出力する
                            // measure 0 は音色/ヘッダセルであり演奏内容ではないためスキップ
                            let wav_ok = if measure > 0 {
                                let wav_path = format!("cmrt/track{}_meas{}.wav", track, measure);
                                crate::pipeline::write_wav(
                                    &samples,
                                    daw_cfg.sample_rate as u32,
                                    &wav_path,
                                ).is_ok()
                            } else {
                                true
                            };
                            // WAV 書き出し失敗はデバッグ出力の問題であり、レンダリング自体は成功している。
                            // そのため WAV 失敗時は Error としてユーザーに通知する。
                            let state = if wav_ok { CacheState::Ready } else { CacheState::Error };
                            cache_worker.lock().unwrap()[track][measure].state = state;
                        }
                        Err(_) => {
                            cache_worker.lock().unwrap()[track][measure].state = CacheState::Error;
                        }
                    }
                }
            });
        }

        let mut app = Self {
            data,
            cursor_track: 0,
            cursor_measure: 0,
            mode: DawMode::Normal,
            textarea: TextArea::default(),
            cfg,
            entry_ptr,
            cache,
            cache_tx,
            render_lock,
            play_state: Arc::new(Mutex::new(DawPlayState::Idle)),
            play_measure_mmls: Arc::new(Mutex::new(vec![String::new(); MEASURES])),
            play_measure_samples: Arc::new(Mutex::new(0)),
        };

        app.load();
        app
    }

    // ─── 保存 / 読み込み ──────────────────────────────────────

    fn load(&mut self) {
        if let Ok(content) = std::fs::read_to_string(DAW_FILE) {
            for (t, track_str) in content.split(';').enumerate() {
                if t >= TRACKS {
                    break;
                }
                for (m, cell) in track_str.split('\n').enumerate() {
                    if m > MEASURES {
                        break;
                    }
                    self.data[t][m] = cell.to_string();
                }
            }
        }
        self.sync_cache_states();
    }

    fn save(&self) {
        let tracks: Vec<String> = self
            .data
            .iter()
            .map(|track| track.join("\n"))
            .collect();
        let _ = std::fs::write(DAW_FILE, tracks.join(";"));
    }

    // ─── キャッシュ管理 ───────────────────────────────────────

    /// data の内容に合わせてキャッシュ状態を同期する（data 変更後に呼ぶ）
    fn sync_cache_states(&self) {
        let mut cache = self.cache.lock().unwrap();
        for t in 0..TRACKS {
            for m in 0..=MEASURES {
                if self.data[t][m].trim().is_empty() {
                    cache[t][m] = CellCache::empty();
                } else if cache[t][m].state == CacheState::Empty {
                    cache[t][m].state = CacheState::Pending;
                }
            }
        }
    }

    /// 指定セルのキャッシュを無効化して状態を更新する
    fn invalidate_cell(&self, track: usize, measure: usize) {
        let mut cache = self.cache.lock().unwrap();
        if self.data[track][measure].trim().is_empty() {
            cache[track][measure] = CellCache::empty();
        } else {
            cache[track][measure] = CellCache { state: CacheState::Pending };
        }
    }

    /// 指定セルのキャッシュジョブをワーカーキューに投入する
    fn kick_cache(&self, track: usize, measure: usize) {
        let mml = self.build_cell_mml(track, measure);
        if mml.trim().is_empty() {
            return;
        }
        // チャネルが既に閉じていれば送信は無視する（DawApp 終了後の残留呼び出しへの安全策）
        let _ = self.cache_tx.send((track, measure, mml));
    }

    /// Pending 状態のすべてのセルをワーカーキューに投入する
    fn kick_all_pending(&self) {
        let pending: Vec<(usize, usize)> = {
            let cache = self.cache.lock().unwrap();
            (0..TRACKS)
                .flat_map(|t| (0..=MEASURES).map(move |m| (t, m)))
                .filter(|&(t, m)| cache[t][m].state == CacheState::Pending)
                .collect()
        };
        for (t, m) in pending {
            self.kick_cache(t, m);
        }
    }

    // ─── MML 構築 ─────────────────────────────────────────────

    /// セル (track, measure) のレンダリング用 MML を構築する
    /// = track[t][0] (音色) + track0 全体 + track[t][m] (音符)
    /// 音色 JSON を先頭に置くことで extract_embedded_json が正しく解析できる
    fn build_cell_mml(&self, track: usize, measure: usize) -> String {
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
    fn build_measure_mmls(&self) -> Vec<String> {
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

    // ─── 演奏 ─────────────────────────────────────────────────

    fn start_play(&self) {
        let measure_mmls = self.build_measure_mmls();
        if measure_mmls.iter().all(|m| m.trim().is_empty()) {
            return;
        }

        // cmrt/ ディレクトリを確保してからデバッグファイルを書き出す
        let _ = crate::pipeline::ensure_cmrt_dir();
        // デバッグ用ファイルに各小節の MML を出力する
        let _ = std::fs::write(DAW_MML_DEBUG_FILE, measure_mmls.join("\n---\n"));

        // play_measure_mmls と play_measure_samples を最新の値で更新してからスレッドに共有する
        *self.play_measure_mmls.lock().unwrap() = measure_mmls;
        *self.play_measure_samples.lock().unwrap() = self.measure_duration_samples();

        let play_state = Arc::clone(&self.play_state);
        let play_measure_mmls = Arc::clone(&self.play_measure_mmls);
        let play_measure_samples = Arc::clone(&self.play_measure_samples);
        let render_lock = Arc::clone(&self.render_lock);
        let cfg = Arc::clone(&self.cfg);
        let entry_ptr = self.entry_ptr;

        *play_state.lock().unwrap() = DawPlayState::Playing;

        std::thread::spawn(move || {
            // SAFETY: entry は main() のスタックに生存している
            let entry_ref: &PluginEntry = unsafe { &*(entry_ptr as *const PluginEntry) };
            let mut daw_cfg = (*cfg).clone();
            daw_cfg.random_patch = false;
            let sample_rate = daw_cfg.sample_rate as u32;

            // OutputStream と Sink をスレッドに 1 つだけ作成し、小節をまたいで再利用する。
            // これにより小節ごとのオーディオ初期化オーバーヘッドとグリッチを防ぐ。
            let Ok((_stream, stream_handle)) = rodio::OutputStream::try_default() else {
                *play_state.lock().unwrap() = DawPlayState::Idle;
                return;
            };
            let Ok(sink) = rodio::Sink::try_new(&stream_handle) else {
                *play_state.lock().unwrap() = DawPlayState::Idle;
                return;
            };

            'outer: loop {
                if *play_state.lock().unwrap() != DawPlayState::Playing {
                    break;
                }
                // ループの先頭で毎回 play_measure_mmls と play_measure_samples を読み取ることで、
                // セル編集・音色変更を次ループから即座に反映する（hot reload）
                let mmls = play_measure_mmls.lock().unwrap().clone();
                let measure_samples = *play_measure_samples.lock().unwrap();

                for mml in &mmls {
                    if *play_state.lock().unwrap() != DawPlayState::Playing {
                        break 'outer;
                    }

                    let samples = if mml.trim().is_empty() {
                        // 空小節: 1小節分の無音を再生して次の小節開始タイミングを保持する
                        vec![0.0f32; measure_samples]
                    } else {
                        // render_lock を取得してからレンダリングすることで、
                        // キャッシュワーカーと同時に cmrt/daw_cache.mid/wav を書き込まないようにする
                        let result = {
                            let _guard = render_lock.lock().unwrap();
                            // mml_render_for_cache を使用することで patch_history.txt への追記を行わない
                            crate::pipeline::mml_render_for_cache(mml, &daw_cfg, entry_ref)
                        };
                        match result {
                            Ok(mut s) => {
                                // 設定された拍子・テンポに基づく 1 小節の長さに正確に pad / truncate する
                                if s.len() < measure_samples {
                                    s.resize(measure_samples, 0.0);
                                } else {
                                    s.truncate(measure_samples);
                                }
                                s
                            }
                            Err(_) => break 'outer,
                        }
                    };

                    if *play_state.lock().unwrap() != DawPlayState::Playing {
                        break 'outer;
                    }
                    // 既存の Sink に追加して再生完了を待つ（OutputStream/Sink は使い回す）
                    let source = rodio::buffer::SamplesBuffer::new(2, sample_rate, samples);
                    sink.append(source);
                    sink.sleep_until_end();
                }
            }

            *play_state.lock().unwrap() = DawPlayState::Idle;
        });
    }

    fn stop_play(&self) {
        *self.play_state.lock().unwrap() = DawPlayState::Idle;
    }

    // ─── ランダム音色 ─────────────────────────────────────────

    fn pick_random_patch_name(&self) -> Option<String> {
        let dir = self.cfg.patches_dir.as_deref()?;
        let patches = collect_patches(dir).ok()?;
        if patches.is_empty() {
            return None;
        }
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0) as usize;
        let idx = ns % patches.len();
        Some(to_relative(dir, &patches[idx]))
    }

    // ─── 描画 ─────────────────────────────────────────────────

    fn draw(&self, f: &mut Frame) {
        ui::draw(self, f);
    }

    // ─── メインループ ─────────────────────────────────────────

    /// TuiApp と同じ terminal を受け取って DAW モードを実行する。
    /// 終了時は `DawExitReason` を返す:
    ///   - `ReturnToTui` : d / ESC キーで TUI に戻る
    ///   - `QuitApp`     : q キーまたは Ctrl+C でアプリを終了する
    pub fn run_with_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<DawExitReason> {
        // Pending セルのキャッシュ構築を開始
        self.kick_all_pending();

        loop {
            terminal.draw(|f| self.draw(f))?;

            if event::poll(std::time::Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    use crossterm::event::KeyEventKind;
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        self.stop_play();
                        return Ok(DawExitReason::QuitApp);
                    }

                    match self.mode {
                        DawMode::Normal => match self.handle_normal(key.code) {
                            DawNormalAction::ReturnToTui => {
                                self.stop_play();
                                return Ok(DawExitReason::ReturnToTui);
                            }
                            DawNormalAction::QuitApp => {
                                self.stop_play();
                                return Ok(DawExitReason::QuitApp);
                            }
                            DawNormalAction::Continue => {}
                        },
                        DawMode::Insert => {
                            self.handle_insert(key);
                        }
                        DawMode::Help => {
                            self.handle_help(key.code);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

