# ADR 0001: プラグインの実測仕様（descriptor と capability）

- 状態: 記録（2026-08-20 実測 / 2026-08-22 に Vaporizer2 を追加して全項目を再 probe）
- 関連: [0002](0002-capability-driven-ports-and-dialects.md) /
  [0006](0006-no-generic-clap-preset-api.md) / [0013](0013-serial-instantiation.md)

## 測り方

**バイナリの文字列検索ではなく実 probe。** サブコマンドが 1 本ある:

```
clap-mml-realtime-play-server probe-capabilities --plugin-path "C:\Program Files\Common Files\CLAP\VASTvaporizer2.clap"
```

`--plugin-path` を省略すると config から引ける全プラグインを 1 回で測る。
実装は `core-lib/src/render/capability_probe.rs` と `realtime-play-server/src/probe.rs`。

**文字列検索で判定してはならない理由**は [0006](0006-no-generic-clap-preset-api.md) の落とし穴を参照。

## 実測値

| | Dexed 1.0.1 | Surge XT 1.3.4 | Vaporizer2 3.5.0 |
|---|---|---|---|
| plugin ID | `com.digital-suburban.dexed` | `org.surge-synth-team.surge-xt` | `com.vastdynamics.VAST2` |
| name / vendor | `Dexed` / `Digital Suburban` | `Surge XT` / `Surge Synth Team` | `Vaporizer2` / `VAST Dynamics` |
| features | `instrument`, `FM`, `DX7` | `instrument`, `synthesizer`, `stereo`, `free and open source` | `instrument` |
| descriptor 数 | 1 | 1 | 1 |
| audio input port | **0** | 1 | 1 |
| audio output port | 1 | **3** | 1 |
| main (port 0) | stereo, `IS_MAIN` | stereo, `IS_MAIN` | stereo, `IS_MAIN` |
| note port | in 1 / out 1 | in 1 / out 0 | in 1 / out 0 |
| note dialect | **MIDI のみ**（supported/preferred `0x2`） | CLAP \| MIDI \| MIDI_MPE | **MIDI のみ** |
| params | 156 | 775 | 755 |
| voice-info | **なし** | あり | **なし** |
| preset-discovery factory | **NULL** | `clap.preset-discovery-factory/2` あり | **NULL** |
| preset-load | **NULL** | `/2` と `.draft/2` の両方あり | **NULL** |
| state / latency / tail / render / gui | あり | あり | あり |
| 受け入れ条件 | OK | OK | OK |

**2026-08-22 の再 probe で 0001 の初版から直した点**: Surge の preset 系を
「draft ID のみ」と書いていたが、実際は **preset-discovery factory も preset-load も
安定 ID（`/2`）が非 NULL**。判断そのもの（[0006](0006-no-generic-clap-preset-api.md)）は
変わらないが、記録としては誤りだった。

## いちばんの落とし穴: Surge の output は 3 本

設計時の資料は「両方 1 本」と書いていたが、それは **Dexed についてのみ正しかった**。
受け入れ条件を「port の本数」で書くと **Surge が起動しなくなる。**

現行の受け入れ条件:

- output port 0 本 → エラー
- 先頭 output port が `IS_MAIN` でない → エラー
- main output が 2ch でない → エラー
- note input port 0 本 → エラー
- dialect が CLAP でも MIDI でもない → エラー

**Vaporizer2 はこの 5 条件を素通りする**ので、追加にあたって `descriptor.rs` の改修は
1 バイトも要らなかった。descriptor も 1 件なので `plugin_id` の指定は必須ではない
（それでも config には書く。将来 descriptor が増えたときに黙って別物を掴まないため）。

## note dialect が MIDI だけのプラグインは voicing probe が成立しない

MIDI dialect には `note_id` が無いので `NOTE_END` が返らない。**Dexed と Vaporizer2 が
これに当たる。** mono/poly の決め方は、プラグインごとに別の材料を使う:

| | mono/poly の材料 |
|---|---|
| Surge XT | CLAP note の `NOTE_END` を数える probe（本来の方法） |
| Dexed | インスタンス設定 `MonoMode` の既定が POLY という実測（[0005](0005-dexed-mono-mode-is-poly.md)） |
| Vaporizer2 | **音色ファイル `.vvp` の `m_uPolyMode`**（[0014](0014-vvp-as-clap-state.md)） |

## 残っている契約違反（承知のうえ）

host は広告された本数ぶんの buffer を渡すべきだが、このコードは以前から
**Surge の 3 本のうち port 0 だけを渡して動いてきた**。直すと Surge の出音が変わりうるので
現行動作を維持している。理由は `core-lib/src/render/descriptor.rs` の
`PluginCapabilities` の doc コメントにもある。

## 壊れたら気づく場所

- `core-lib/src/render/tests/surge.rs::surge_still_advertises_a_stereo_main_output_and_the_clap_note_dialect`
  — 落ちたら Surge が MIDI dialect 経路へ落ちている（出音が変わりうる）
- `core-lib/src/render/tests/vaporizer2.rs::vaporizer2_advertises_a_stereo_main_output_and_only_the_midi_note_dialect`
  — 落ちたら Vaporizer2 の受け入れ条件・dialect が変わった（voicing の材料の前提が崩れる）
