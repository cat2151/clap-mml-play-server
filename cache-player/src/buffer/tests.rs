use std::sync::Arc;

use crate::buffer::{CacheBuffer, VoiceBank, MAX_VOICES};
use crate::graveyard::BufferGraveyard;

fn buffer(frames: usize) -> Arc<CacheBuffer> {
    Arc::new(CacheBuffer::from_channels(vec![vec![1.0; frames]], 48_000))
}

/// 鳴り終わった voice の `Arc` を drop せず graveyard へ渡すこと。
///
/// ここが崩れると、最後の参照が消えた瞬間に数 MB の解放が `process` の中で走る。
#[test]
fn a_finished_voice_hands_its_buffer_to_the_graveyard() {
    let mut graveyard = BufferGraveyard::new();
    let mut bank = VoiceBank::new();
    let source = buffer(4);

    bank.note_on(Arc::clone(&source), &mut graveyard);
    bank.advance(4, &mut graveyard);

    assert!(!bank.has_active_voices(), "voice が終わっていない");
    assert_eq!(graveyard.len(), 1, "graveyard へ渡っていない");
    assert_eq!(
        Arc::strong_count(&source),
        2,
        "RT スレッド側で解放されている"
    );
}

/// 空きが無くて潰された voice の `Arc` も graveyard へ渡ること。
#[test]
fn a_stolen_voice_hands_its_buffer_to_the_graveyard() {
    let mut graveyard = BufferGraveyard::new();
    let mut bank = VoiceBank::new();
    let source = buffer(1_000);

    for _ in 0..MAX_VOICES {
        bank.note_on(Arc::clone(&source), &mut graveyard);
    }
    assert_eq!(graveyard.len(), 0, "空きがあるうちに潰している");

    bank.note_on(Arc::clone(&source), &mut graveyard);
    assert_eq!(
        graveyard.len(),
        1,
        "潰した voice が graveyard へ渡っていない"
    );
}

/// `stop_all` も同じく graveyard 経由であること（`stop_processing` から呼ばれる）。
#[test]
fn stop_all_hands_every_buffer_to_the_graveyard() {
    let mut graveyard = BufferGraveyard::new();
    let mut bank = VoiceBank::new();
    let source = buffer(1_000);

    bank.note_on(Arc::clone(&source), &mut graveyard);
    bank.note_on(Arc::clone(&source), &mut graveyard);
    bank.stop_all(&mut graveyard);

    assert!(!bank.has_active_voices());
    assert_eq!(graveyard.len(), 2);
    assert_eq!(Arc::strong_count(&source), 3, "解放されてしまっている");
}
