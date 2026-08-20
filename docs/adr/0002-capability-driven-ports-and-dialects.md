# ADR 0002: audio port / note dialect は capability 駆動で決める

- 状態: 採用（2026-08-20）
- 関連: [0001](0001-measured-plugin-capabilities.md) / [0011](0011-clack-host-notes.md)

## 決定

instance 生成直後（`activate()` の**前**、main thread）に extension を読み、
`PluginCapabilities` に保持する。プラグインごとの分岐はここ 1 か所へ閉じる。

- `audio_input_ports` が 0 なら **input buffer を渡さない**
- `input_note_dialects` が CLAP を含めば CLAP `NoteOnEvent` / `NoteOffEvent`
- 含まず MIDI を含めば **3-byte MIDI へ変換して `ClapMidiEvent` を push**
- どちらも無ければエラー

live 経路は元から `ClapMidiEvent` なので無改修だった。

## MIDI へ落とす変換式

```
NoteOn  { channel, key, velocity } → [0x90 | (channel & 0x0F), key & 0x7F, velocity & 0x7F]
NoteOff { channel, key, velocity } → [0x80 | (channel & 0x0F), key & 0x7F, velocity & 0x7F]
```

## 帰結: port 構成が instance ごとに違っても既に動く

capability の差は **instance 単位で吸収されている**（`core-lib/src/render/descriptor.rs`
`probe_capabilities()`）。audio input 0 本、MIDI-only dialect といった差は
`PluginCapabilities` が instance ごとに持つので、**混在は port 構成の面では追加作業ゼロだった。**
