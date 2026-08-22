//! 演奏中の跨ぎ差し替えが壊れないか。**実プラグインが要る**（回し方は親モジュールの doc）。
//!
//! ここが黒なら予備プール方式（ADR 0008）と unsafe な移送（ADR 0009）ごと成立しない。

use super::*;

/// **予備プール方式の本丸。** 演奏しながら背景でインスタンスを作り続け、プラグインを
/// またいで差し替え続けても落ちないか。`RendererHandoff` の unsafe な移送を
/// 演奏中ずっと踏むことになるので、ここが黒なら予備プール方式ごと成立しない。
#[test]
#[ignore = "実プラグインが要る"]
fn swapping_plugins_under_a_running_render_loop_survives_many_cycles() {
    const CYCLES: usize = 20;
    const SLOTS: usize = 4;
    let cartridge = first_cartridge_patch();
    let kinds = real_kinds();
    let mut renderers = create_live_renderers(&kinds[0], SLOTS).unwrap();
    let mut instances = LiveInstances::new(kinds, SLOTS);

    let started = Instant::now();
    for cycle in 0..CYCLES {
        for slot in 0..SLOTS {
            // 偶数周は Dexed の cartridge、奇数周は音色無指定（既定の Surge XT）。
            let patch = (cycle % 2 == 0).then_some(cartridge.as_str());
            prepare(&mut instances, &mut renderers, slot, patch);
            assert!(
                render_note(&mut renderers[slot]) > 0.0,
                "cycle {cycle} slot {slot} が差し替え後に無音になった"
            );
        }
        // 差し替えの合間も演奏は続いている。背景生成と並走させる。
        for renderer in renderers.iter_mut() {
            render_idle(renderer, 4);
        }
        instances.collect_ready();
    }
    eprintln!(
        "swap cycles={CYCLES} slots={SLOTS} total_ms={} physical={}",
        started.elapsed().as_millis(),
        instances.physical_count()
    );
    drop(renderers);
    drop(instances);
}

/// 差し替えた行の音が破綻しないか。袋へ返す前に音を止めていないと、
/// 返した物理インスタンスが鳴りっぱなしのまま次の行で使われる。
#[test]
#[ignore = "実プラグインが要る"]
fn a_note_left_sounding_does_not_survive_the_trip_through_the_pool() {
    let cartridge = first_cartridge_patch();
    let kinds = real_kinds();
    let mut renderers = create_live_renderers(&kinds[0], 1).unwrap();
    let mut instances = LiveInstances::new(kinds, 1);

    // Surge の音を鳴らしっぱなしにしたまま Dexed へ差し替える。
    prepare(&mut instances, &mut renderers, 0, None);
    let _ = renderers[0].render_live_chunk(&[[0x90, 60, 100]]).unwrap();
    assert!(
        render_idle(&mut renderers[0], 4) > 0.0,
        "差し替え前から無音になっている"
    );
    prepare(&mut instances, &mut renderers, 0, Some(&cartridge));
    // 同じ Surge インスタンスが袋から戻ってくる（袋は LIFO）。
    prepare(&mut instances, &mut renderers, 0, None);

    let leaked = render_idle(&mut renderers[0], 16);

    assert!(leaked < 1.0e-4, "前の音が残っている: peak={leaked}");
}

/// **ADR 0009 の賭けを 3 プラグインで取り直す。** 2 プラグイン（Surge XT / Dexed）でしか
/// 測っていなかった「演奏中に `RendererHandoff` の unsafe な移送を踏み続けて大丈夫か」を、
/// Vaporizer2 を混ぜた構成でもう一度殴る。
///
/// Vaporizer2 は **instance のスレッド並列生成で segfault する**唯一の実例なので、
/// ここが通ることは同時に「背景スレッドでの生成がホスト側の直列化で守られている」
/// ことの確認でもある（守られていなければ、背景生成と worker の生成が重なった瞬間に
/// プロセスごと落ちる）。
#[test]
#[ignore = "実プラグインが要る"]
fn swapping_across_three_plugins_under_a_running_render_loop_survives_many_cycles() {
    const CYCLES: usize = 15;
    const SLOTS: usize = 4;
    let cartridge = first_cartridge_patch();
    let vvp = first_vvp_patch();
    let kinds = real_kinds_with_vaporizer2();
    let mut renderers = create_live_renderers(&kinds[0], SLOTS).unwrap();
    let mut instances = LiveInstances::new(kinds, SLOTS);

    let started = Instant::now();
    for cycle in 0..CYCLES {
        for slot in 0..SLOTS {
            // 3 周で一巡: 既定（Surge XT）→ Dexed の cartridge → Vaporizer2 の `.vvp`。
            // slot ごとにずらして、同じ周でも行によって別プラグインが載っている状態を作る。
            let patch = match (cycle + slot) % 3 {
                0 => None,
                1 => Some(cartridge.as_str()),
                _ => Some(vvp.as_str()),
            };
            prepare(&mut instances, &mut renderers, slot, patch);
            assert!(
                render_note(&mut renderers[slot]) > 0.0,
                "cycle {cycle} slot {slot} が差し替え後に無音になった"
            );
        }
        // 差し替えの合間も演奏は続いている。背景生成と並走させる。
        for renderer in renderers.iter_mut() {
            render_idle(renderer, 4);
        }
        instances.collect_ready();
    }
    eprintln!(
        "three_plugin_swap cycles={CYCLES} slots={SLOTS} total_ms={} physical={} working_set={}",
        started.elapsed().as_millis(),
        instances.physical_count(),
        working_set_mb(),
    );
    drop(renderers);
    drop(instances);
}
