//! cartridge program の切替（Phase 2）。
//!
//! Dexed の音色を「cartridge の 1 program」で選ぶ経路を、実プラグインと実物の
//! cartridge で確かめる。共通のヘルパは親モジュールにある。

use super::*;

const DEXED_CARTRIDGES_ENV: &str = "CMRT_TEST_DEXED_CARTRIDGES";

fn cartridges_dir() -> String {
    std::env::var(DEXED_CARTRIDGES_ENV).unwrap_or_else(|_| {
        panic!("{DEXED_CARTRIDGES_ENV} に Dexed の Cartridges ディレクトリを設定すること")
    })
}

/// 実物の cartridge から patch path を拾う。
///
/// `collect_patches` は cartridge ごとに 32 件を連続して並べるので、先頭 2 件は
/// 同じ cartridge の program 0 と 1 になる。
fn installed_cartridge_programs() -> Vec<String> {
    let patches = crate::collect_patches(&cartridges_dir()).unwrap();
    assert!(
        patches.len() >= 64,
        "cartridge が 2 個以上ある状態で実行すること（{} 件）",
        patches.len()
    );
    patches
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// 先頭 cartridge の program 0 / 1 と、別 cartridge の program 0。
fn three_test_programs() -> (String, String, String) {
    let programs = installed_cartridge_programs();
    let first_cartridge = std::path::Path::new(&programs[0])
        .parent()
        .unwrap()
        .to_path_buf();
    let other = programs
        .iter()
        .find(|path| std::path::Path::new(path).parent() != Some(first_cartridge.as_path()))
        .expect("cartridge が 1 個しかない")
        .clone();
    (programs[0].clone(), programs[1].clone(), other)
}

fn dexed_renderer() -> (PluginEntry, RealtimeRenderer) {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let renderer = RealtimeRenderer::new(&test_config(), &entry).unwrap();
    (entry, renderer)
}

/// patch を選び直してから 1 音鳴らす。比較のため毎回 `reset()` を挟む。
fn render_program(renderer: &mut RealtimeRenderer, patch: &str) -> Vec<f32> {
    renderer.set_patch(Some(patch)).unwrap();
    renderer.reset();
    render_live_note(renderer)
}

fn max_abs_difference(left: &[f32], right: &[f32]) -> f32 {
    assert_eq!(left.len(), right.len());
    left.iter()
        .zip(right)
        .map(|(left, right)| (left - right).abs())
        .fold(0.0_f32, f32::max)
}

/// 「同じ音」と見なす差。
///
/// サンプル完全一致は使えない。同じ program を選び直しても Dexed の内部状態
/// （LFO の位相など）までは戻らず、実測で 2e-5 程度の差が残るため。
/// 別 program との差は 0.3 前後なので、この閾値で十分に分かれる。
const SAME_SOUND_TOLERANCE: f32 = 0.001;
/// 「別の音」と見なす最小の差。取り違えを見逃さないよう、同一判定の 100 倍を要求する。
const DIFFERENT_SOUND_THRESHOLD: f32 = 0.1;

#[test]
#[ignore = "実プラグインと実物の cartridge が要る"]
fn dexed_renders_a_selected_cartridge_program() {
    let (_entry, mut renderer) = dexed_renderer();
    let (program_00, _, _) = three_test_programs();

    let samples = render_program(&mut renderer, &program_00);

    assert!(peak(&samples) > 0.0, "cartridge program が無音になっている");
}

/// SysEx + Program Change が実際に効いていること。ここが効いていないと、どの program を
/// 選んでも cartridge の program 0 が鳴り続ける（一覧だけ増えて音は 1 つ）。
#[test]
#[ignore = "実プラグインと実物の cartridge が要る"]
fn dexed_program_change_actually_changes_the_sound() {
    let (_entry, mut renderer) = dexed_renderer();
    let (program_00, program_01, _) = three_test_programs();

    let first = render_program(&mut renderer, &program_00);
    let second = render_program(&mut renderer, &program_01);

    assert!(peak(&first) > 0.0, "program 00 が無音");
    assert!(peak(&second) > 0.0, "program 01 が無音");
    let difference = max_abs_difference(&first, &second);
    assert!(
        difference > DIFFERENT_SOUND_THRESHOLD,
        "program を変えても出音がほぼ同じ（最大差 {difference}）"
    );
}

/// 同じ program を選び直したら同じ音になること。
///
/// 同一 cartridge 内の移動では SysEx を送り直さない最適化が入っているので、
/// 「送り直さなかったせいで前の program のままだった」をここで弾く。
#[test]
#[ignore = "実プラグインと実物の cartridge が要る"]
fn dexed_reselecting_a_program_reproduces_the_same_sound() {
    let (_entry, mut renderer) = dexed_renderer();
    let (program_00, program_01, _) = three_test_programs();

    let first = render_program(&mut renderer, &program_01);
    let _ = render_program(&mut renderer, &program_00);
    let again = render_program(&mut renderer, &program_01);

    let difference = max_abs_difference(&first, &again);
    assert!(
        difference < SAME_SOUND_TOLERANCE,
        "同じ program を選び直したのに音が違う（最大差 {difference}）"
    );
}

#[test]
#[ignore = "実プラグインと実物の cartridge が要る"]
fn dexed_switches_between_cartridges() {
    let (_entry, mut renderer) = dexed_renderer();
    let (program_00, _, other_cartridge) = three_test_programs();

    let first = render_program(&mut renderer, &program_00);
    let other = render_program(&mut renderer, &other_cartridge);

    assert!(peak(&other) > 0.0, "別 cartridge が無音になっている");
    let difference = max_abs_difference(&first, &other);
    assert!(
        difference > DIFFERENT_SOUND_THRESHOLD,
        "cartridge を変えても出音がほぼ同じ（最大差 {difference}）"
    );
}

/// `set_patch(None)` は CLAP state load で、Dexed はその直後 約 2 秒 program change を
/// 無視する。cartridge を選び直したときに guard へ当たっていないことを実測で固定する。
#[test]
#[ignore = "実プラグインと実物の cartridge が要る"]
fn dexed_selects_the_right_program_right_after_returning_to_the_initial_voice() {
    let (_entry, mut renderer) = dexed_renderer();
    let (_, program_01, _) = three_test_programs();

    let before = render_program(&mut renderer, &program_01);
    renderer.set_patch(None).unwrap();
    let after = render_program(&mut renderer, &program_01);

    let difference = max_abs_difference(&before, &after);
    assert!(
        difference < SAME_SOUND_TOLERANCE,
        "初期音色へ戻した直後の program 切替が効いていない（最大差 {difference}）。         cartridge + Program Change 方式に戻すと Dexed の 2 秒 guard でここが落ちる"
    );
}

/// cartridge を指定して作った renderer が、生成直後からその program で鳴ること。
/// オフライン render は 1 回ごとに instance を作るので、この経路が本番。
#[test]
#[ignore = "実プラグインと実物の cartridge が要る"]
fn dexed_offline_render_honors_the_configured_cartridge_program() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let (program_00, program_01, _) = three_test_programs();
    let cfg_00 = CoreConfig {
        patch_path: Some(program_00),
        ..test_config()
    };
    let cfg_01 = CoreConfig {
        patch_path: Some(program_01),
        ..test_config()
    };

    let first = mml_render_stateless("t120 o4 l4 c", &cfg_00, &entry).unwrap();
    let second = mml_render_stateless("t120 o4 l4 c", &cfg_01, &entry).unwrap();

    assert!(peak(&first) > 0.0, "program 00 のオフライン render が無音");
    assert!(peak(&second) > 0.0, "program 01 のオフライン render が無音");
    let difference = max_abs_difference(&first, &second);
    assert!(
        difference > DIFFERENT_SOUND_THRESHOLD,
        "オフライン render で program が効いていない（最大差 {difference}）"
    );
}

/// 壊れた patch path を黙って初期音色で鳴らさない。
#[test]
#[ignore = "実プラグインが要る"]
fn dexed_rejects_a_cartridge_patch_path_with_a_bad_program_component() {
    let (_entry, mut renderer) = dexed_renderer();

    let error = renderer
        .set_patch(Some("Dexed_01.syx/99 Nope"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("99 Nope"), "{error}");
}

#[test]
#[ignore = "実プラグインが要る"]
fn dexed_reports_a_missing_cartridge_file_with_its_path() {
    let (_entry, mut renderer) = dexed_renderer();

    let error = format!(
        "{:#}",
        renderer
            .set_patch(Some("does/not/exist.syx/00 Init"))
            .unwrap_err()
    );

    assert!(error.contains("exist.syx"), "{error}");
}

/// 報告書 5 章が実証した手順（cartridge 4,104 bytes を送ってから Program Change）。
///
/// 本番はこれを使わず single voice SysEx で送る（理由は [`super::cartridge_patch`]）。
/// packed voice の展開を間違えると「別の音だが鳴ってはいる」という一番気づきにくい
/// 壊れ方をするので、この参照実装との聴き比べをテストで固定する。
fn select_program_with_cartridge_and_program_change(
    renderer: &mut RealtimeRenderer,
    cartridge_path: &str,
    program_index: u8,
) {
    use clack_host::events::event_types::MidiSysExEvent;

    let bytes = std::fs::read(cartridge_path).unwrap();
    let mut events = EventBuffer::new();
    // SAFETY: `process()` が返るまで `bytes` を生かしておく。
    let sysex = unsafe { MidiSysExEvent::new(0, 0, &bytes) };
    events.push(&sysex);
    renderer
        .process_chunk_with_events(renderer.buf_size as u32, &events)
        .unwrap();
    drop(bytes);

    let mut events = EventBuffer::new();
    events.push(&ClapMidiEvent::new(0, 0, [0xC0, program_index, 0]));
    renderer
        .process_chunk_with_events(renderer.buf_size as u32, &events)
        .unwrap();
    let empty = EventBuffer::new();
    renderer
        .process_chunk_with_events(renderer.buf_size as u32, &empty)
        .unwrap();
}

/// packed voice の展開が正しいこと。
///
/// single voice SysEx で選んだ音と、報告書の手順（cartridge + Program Change）で
/// 選んだ音が一致することで確かめる。bit の割り当てを 1 つでも間違えれば別の音になる。
#[test]
#[ignore = "実プラグインと実物の cartridge が要る"]
fn dexed_single_voice_sysex_sounds_like_the_program_change_reference() {
    let programs = installed_cartridge_programs();
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();

    // 同じ cartridge の複数 program で確かめる。1 つだけだと bit 割り当ての
    // 取り違えがたまたま出ない program を引きうる。
    for program in [0usize, 1, 7, 31] {
        let patch = &programs[program];
        let cartridge = std::path::Path::new(patch)
            .parent()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        let mut mine = RealtimeRenderer::new(&test_config(), &entry).unwrap();
        let mine_samples = render_program(&mut mine, patch);

        let mut reference = RealtimeRenderer::new(&test_config(), &entry).unwrap();
        select_program_with_cartridge_and_program_change(&mut reference, &cartridge, program as u8);
        reference.reset();
        let reference_samples = render_live_note(&mut reference);

        assert!(peak(&mine_samples) > 0.0, "program {program} が無音");
        let difference = max_abs_difference(&mine_samples, &reference_samples);
        assert!(
            difference < SAME_SOUND_TOLERANCE,
            "program {program}: single voice SysEx と Program Change で音が違う（最大差 {difference}）"
        );
    }
}
