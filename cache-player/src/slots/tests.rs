use std::path::Path;

use super::*;

#[test]
fn a_bare_path_is_slot_zero() {
    // スロット番号なしの綴り（cache-player を入れたときの形）が壊れないこと。
    assert_eq!(
        parse_state(r"C:\cache\track2_meas1.wav").unwrap(),
        StateRequest::Load {
            slot: 0,
            path: r"C:\cache\track2_meas1.wav".to_string(),
        }
    );
}

#[test]
fn a_slot_prefix_selects_the_slot() {
    assert_eq!(
        parse_state(r"slot=1;C:\cache\track2_meas3.wav").unwrap(),
        StateRequest::Load {
            slot: 1,
            path: r"C:\cache\track2_meas3.wav".to_string(),
        }
    );
}

#[test]
fn an_empty_state_clears_every_slot() {
    assert_eq!(parse_state("   ").unwrap(), StateRequest::ClearAll);
}

#[test]
fn an_empty_path_clears_just_that_slot() {
    assert_eq!(
        parse_state("slot=1;").unwrap(),
        StateRequest::Clear { slot: 1 }
    );
}

#[test]
fn a_broken_slot_spelling_is_an_error_not_a_silent_slot_zero() {
    // 黙って 0 へ落とすと「載せたはずの小節が鳴らない」が原因不明の無音になる。
    assert!(parse_state("slot=9;C:/x.wav").is_err());
    assert!(parse_state("slot=x;C:/x.wav").is_err());
    assert!(parse_state("slot=1").is_err());
}

/// 綴りを変えても patch 文字列の拡張子判定（`core-lib/src/cache_wav.rs`）が
/// 壊れないこと。サフィックス形にすると壊れるので、プレフィクス形に決めてある。
#[test]
fn the_spelling_keeps_the_wav_extension_visible() {
    let patch = slot_patch_state(1, r"C:\cache\track2_meas3.wav");

    assert_eq!(
        Path::new(&patch).extension().and_then(|e| e.to_str()),
        Some("wav"),
        "{patch}"
    );
}

#[test]
fn note_numbers_wrap_around_the_slots() {
    assert_eq!(slot_for_note(60), 0, "従来の note 60 はスロット 0 のまま");
    for slot in 0..SLOT_COUNT {
        assert_eq!(
            slot_for_note(60 + slot as u8),
            slot,
            "note 60 + s はスロット s（60 が SLOT_COUNT の倍数だから成り立つ）"
        );
    }
    assert_eq!(
        slot_for_note(60 + SLOT_COUNT as u8),
        0,
        "範囲を超えた note は剰余で巻き戻る（黙って無音にしない）"
    );
}

/// **スロット数は、演奏ループがサーバーのクロックより先行できる小節数を決める。**
///
/// DAW は 1 小節先まで先読みするので、先行が `SLOT_COUNT` 小節に届いた時点で
/// 「まだ鳴っていない小節」のスロットを踏み潰す（＝違う小節が鳴る）。実測の先行は
/// 1.3 小節（`clap-mml-render-tui` の `docs/adr/0012-live-clock-drift-is-absorbed-not-eliminated.md`）なので、2 本では足りない。
#[test]
fn the_slot_count_leaves_room_for_a_clock_drift_of_three_measures() {
    // 本数は定数なので、減らしたら**このテスト crate がコンパイルできなくなる**。
    // 「実行したら落ちる」より早く気づける側へ倒してある。
    const {
        assert!(
            SLOT_COUNT >= 4,
            "先読みの余裕は SLOT_COUNT - 1 小節。実測 1.3 小節の先行を吸収するには 4 本要る"
        )
    };
    assert_eq!(60 % SLOT_COUNT, 0, "note 60 + slot の対応が崩れる");
}
