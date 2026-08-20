# ADR 0001: プラグインの実測仕様（descriptor と capability）

- 状態: 記録（2026-08-20 実測）
- 関連: [0002](0002-capability-driven-ports-and-dialects.md)

## 実測値

| | Dexed 1.0.1 | Surge XT 1.3.4 |
|---|---|---|
| plugin ID | `com.digital-suburban.dexed` | `org.surge-synth-team.surge-xt` |
| name / vendor | `Dexed` / `Digital Suburban` | — |
| features | `instrument`, `FM`, `DX7` | — |
| descriptor 数 | 1 | 1 |
| audio input port | **0** | 1 |
| audio output port | 1 | **3** |
| main (port 0) | stereo, `IS_MAIN` | stereo, `IS_MAIN` |
| note port | in 1 / out 1 | in 1 |
| note dialect | **MIDI のみ**（supported/preferred `0x2`） | CLAP \| MIDI |
| params | 156 | — |
| voice-info | **なし** | あり |
| preset-load | **なし** | draft ID のみ |
| state / latency / tail | あり | あり |

## いちばんの落とし穴: Surge の output は 3 本

設計時の資料は「両方 1 本」と書いていたが、それは **Dexed についてのみ正しかった**。
受け入れ条件を「port の本数」で書くと **Surge が起動しなくなる。**

現行の受け入れ条件:

- output port 0 本 → エラー
- 先頭 output port が `IS_MAIN` でない → エラー
- main output が 2ch でない → エラー
- note input port 0 本 → エラー
- dialect が CLAP でも MIDI でもない → エラー

## 残っている契約違反（承知のうえ）

host は広告された本数ぶんの buffer を渡すべきだが、このコードは以前から
**Surge の 3 本のうち port 0 だけを渡して動いてきた**。直すと Surge の出音が変わりうるので
現行動作を維持している。理由は `core-lib/src/render/descriptor.rs` の
`PluginCapabilities` の doc コメントにもある。

## 壊れたら気づく場所

- `core-lib/src/render/tests.rs::surge_still_advertises_a_stereo_main_output_and_the_clap_note_dialect`
  — 落ちたら Surge が MIDI dialect 経路へ落ちている（出音が変わりうる）
