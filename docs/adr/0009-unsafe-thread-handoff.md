# ADR 0009: unsafe thread handoff は測定で受け入れている（証明ではない）

- 状態: 記録（判定: 白）
- 関連: [0008](0008-spare-instance-pool.md) / [0012](0012-measured-baselines.md) /
  [0013](0013-serial-instantiation.md)

## 何を踏んでいるか

`core-lib/src/render/parallel.rs` は CLAP の
**「`init()` したスレッドと `clap_plugin_state.load` を呼ぶスレッドは同じ main thread」**
という規約を**意図的に破っている**。`!Send` な instance を unsafe に別スレッドへ移送する
（`RendererHandoff`）。

**予備インスタンスプール（[0008](0008-spare-instance-pool.md)）は、これを起動時 1 回ではなく
演奏中ずっと踏む。**

## 実測の結果: 白

演奏しながらの跨ぎ差し替えを、2 プラグイン（Surge XT ⇄ Dexed）で **160 回**、
3 プラグイン（Surge XT の `.fxp` ⇄ Dexed の cartridge ⇄ Vaporizer2 の `.vvp` を行ごとにずらして
15 周 × 4 行 = **60 回**）で通し、**全回で差し替え後に音が出ている**（無音判定つき）。
差し替え自体は `ms=0`（袋から取るだけ）。

## 賭けが外れた 1 例目: **Vaporizer2 は instance の並列生成で落ちる**

thread handoff そのものではないが、**「プラグインは host のスレッド規約を多少破っても動く」
という賭けが実際に外れた 1 例目**なので、ここに残す。

```
cargo run --release --example parallel_instance_creation -- "<VASTvaporizer2.clap>" 8
```

Vaporizer2 3.5.0 は直列 8 個なら OK、並列（entry 共有・複数スレッド）だと
**segfault（STATUS_ACCESS_VIOLATION）**。2 スレッドでも落ち、間欠ではない。
Dexed 1.0.1 と Surge XT 1.3.4 は並列でも OK。

- entry を 1 つに共有しても落ちるので `PluginEntry::load` の競合ではなく**生成そのもの**
- 対策は [0013](0013-serial-instantiation.md)（プラグイン別に生成を直列化）。
  **上の 60 回はその対策が入った状態の数字**で、外すと同じ経路が落ちる

## ただし、これは測定であって証明ではない

- 成功は **Windows / 48kHz / buffer 512** で、**Dexed v1.0.1 / Surge XT 1.3.4 / Vaporizer2 3.5.0**
  の組み合わせで測った結果にすぎない
- **プラグインのバージョンが上がったら取り直すこと。** plugin / version 別の回帰対象として扱う
- **対応プラグインを増やしたら、まず `examples/parallel_instance_creation.rs` を通すこと。**
  Vaporizer2 がそこで落ちた
- **型では保証されない。** コンパイラは助けてくれない
- 16 instance の並列生成テストの直後にテストプロセスごと異常終了した例が 1 度だけあり、
  再現せず原因未特定（port / SHM の取り合いを疑っている）

## 壊れたら気づく場所

- `player::instances::tests::swapping::swapping_plugins_under_a_running_render_loop_survives_many_cycles`
  — 演奏中の跨ぎ差し替え（2 プラグイン）
- `player::instances::tests::swapping::swapping_across_three_plugins_under_a_running_render_loop_survives_many_cycles`
  — 同（3 プラグイン。**背景スレッドの生成と worker の生成が重なっても落ちないこと**まで見る）
- `core-lib/src/render/tests/dexed.rs::dexed_survives_parallel_creation_handoff_and_playback_of_sixteen_instances`
  — 起動時の並列生成
- `cargo run --release --example parallel_instance_creation -- "<CLAP>" 8` の**終了コード**
  — 新しいプラグインが並列生成に耐えるか（0 / 139）
