# ADR 0011: clack / host 実装の知識

- 状態: 記録（2026-08-20）
- 関連: [0002](0002-capability-driven-ports-and-dialects.md)

## 実装で効いている性質

- **空 input port で `process()` は正しく動く。** clack の
  `host/src/process/audio_buffers.rs:252` で、port が 0 件なら `frames_count: None` になり、
  `min_available_frames_with()` が output 側の frames を採用する（Dexed は audio input 0）
- **`EventFlags::IS_LIVE` は live 経路専用。** offline では付けない
  （オフラインレンダリングの再現性のため）
- **MIDI dialect には `note_id` が無いので `NoteEnd` が返らない。**
  voicing probe は CLAP note に `note_id` を載せて NoteOn を送り、返る `NOTE_END` の数で
  voice 数を決めているので、Dexed では**永久に返らず常に Poly を返す**。
  「判定できていない」と「Poly と判定した」が見分けられなくなるので、
  **dialect が CLAP を含まない場合は probe を実行しない**

## 参照 rev / 外部資料

- clack `c5975f9f89f0953b00768680357985d46178078a`
  （`extensions/src/note_ports.rs` / `note_ports/host.rs` / `audio_ports/host.rs` /
  `host/src/process/audio_buffers.rs:145-257`）
- Dexed v1.0.1 の `CMakeLists.txt` / `Source/PluginProcessor.cpp`
- Dexed が pin する JUCE CLAP wrapper
  `clap-juce-extensions@4d454e5125da75a0e75d95615cbec26d2a09e2bf`
