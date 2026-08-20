# ADR 0003: Dexed の音色変更は single voice SysEx で送る

- 状態: 採用（2026-08-20）
- 関連: [0004](0004-syx-format-and-persistent-ids.md) / [0008](0008-spare-instance-pool.md)

## 実測した現象

**Dexed v1.0.1 は CLAP state load の直後 約 2 秒、host からの Program Change を捨てる。**

cartridge SysEx 自体は通るので、**「cartridge はロードされ、program 0 のまま鳴る」という
いちばん気づきにくい壊れ方**をする。

| 手順 | 結果 |
|---|---|
| program 01 を選ぶ → 発音 | peak 0.257 / rms 0.0518 |
| `set_patch(None)`（= CLAP state load）→ program 01 を選び直す → 発音 | peak 0.126 / rms 0.0709 = **program 00 の音**（差 0.00034） |

## 決定: single voice SysEx

cartridge から目的の voice だけを取り出し、**1 voice ぶんの SysEx（163 bytes、format 0）**として
送って Dexed の edit buffer を直接書く。

**Program Change を使わないので guard と無関係になる。**
`set_patch(None)` の意味（生成直後の state へ戻す）も Surge と揃ったまま変えずに済む。

## 採らなかった案

**「Dexed の `None` を初期 program へ正規化して state load 経路を排除する」**:
`patch_path` を書いていない config では `None` の行き先が無く**正規化できない**。
grid sequencer や notepad の「音色指定の無い行」は普通にあるので、この穴は実運用で踏む。

**リアルタイム処理を 2 秒 block / sleep して隠すことは禁止**（`AGENTS.md` のデバウンス禁止と同趣旨）。

## 罠

- **`set_patch(None)` は state load なので、呼ぶたびに 2 秒 guard が armed になる。**
  いまは Program Change を使わないので無害だが、**Program Change を使う実装を足すと即座に壊れる**
- 予備インスタンスプールへ返すときの初期化がこれを踏む（[0008](0008-spare-instance-pool.md)）。
  「全 instance へ一斉に初期化を投げる」経路があると Dexed 側だけ取りこぼす
- **SysEx の backing buffer は `process()` が返るまで生かす。** `MidiSysExEvent::new` は `unsafe` で
  buffer を借用するだけ。`render/cartridge_patch.rs` では明示的に `drop` を後ろへ置いてある
- **cartridge patch は `activate()` 前に送れない。** SysEx は event なので `process()` が要る。
  `RealtimeRenderer::new_with_timing` は `start_processing()` のあとで `set_patch()` を呼ぶ

## 壊れたら気づく場所

- `core-lib/src/render/tests/cartridge.rs::dexed_single_voice_sysex_sounds_like_the_program_change_reference`
  — packed voice の展開を Program Change の参照手順と突き合わせる（program 0 / 1 / 7 / 31 で最大差 < 0.001）。
  **bit の割り当てを 1 つでも間違えれば「鳴ってはいるが別の音」になる**ので、
  参照実装との比較をテストに残してある
