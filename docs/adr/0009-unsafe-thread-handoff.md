# ADR 0009: unsafe thread handoff は測定で受け入れている（証明ではない）

- 状態: 記録（2026-08-20 実測 / 判定: 白。2026-08-22 に 3 プラグインで取り直し）
- 関連: [0008](0008-spare-instance-pool.md) / [0012](0012-measured-baselines.md) /
  [0013](0013-serial-instantiation.md)

## 何を踏んでいるか

`core-lib/src/render/parallel.rs` は CLAP の
**「`init()` したスレッドと `clap_plugin_state.load` を呼ぶスレッドは同じ main thread」**
という規約を**意図的に破っている**。`!Send` な instance を unsafe に別スレッドへ移送する
（`RendererHandoff`、旧 `SendRenderer`）。

**予備インスタンスプール（[0008](0008-spare-instance-pool.md)）は、これを起動時 1 回ではなく
演奏中ずっと踏む。** これが混在計画で唯一の未知だった。

## 実測の結果: 白

**160 回の跨ぎ差し替えを演奏しながら通して異常なし。** 予備プールは成立する。

## 取り直し（2026-08-22 / Vaporizer2 を足した 3 プラグイン構成）

`swapping_across_three_plugins_under_a_running_render_loop_survives_many_cycles`
（Surge XT の `.fxp` ⇄ Dexed の cartridge ⇄ Vaporizer2 の `.vvp` を行ごとにずらして
15 周 × 4 行 = **60 回**）:

```
three_plugin_swap cycles=15 slots=4 total_ms=1151 physical=16 working_set=765,820 K
```

- **60 回すべてで差し替え後に音が出ている**（無音判定つき）。差し替え自体は `ms=0`（袋から取るだけ）
- 既存の 2 プラグイン版（`swap cycles=20 slots=4 total_ms=85 physical=12`）も通ったまま
- **判定は引き続き白。** ただし下の「賭けが外れた 1 例目」を読むこと

## 賭けが外れた 1 例目: **Vaporizer2 は instance の並列生成で落ちる**

thread handoff そのものではないが、**「プラグインは host のスレッド規約を多少破っても動く」
という賭けが実際に外れた 1 例目**なので、ここに残す。

```
cargo run --release --example parallel_instance_creation -- "<VASTvaporizer2.clap>" 8
```

| プラグイン | 直列 8 個 | 並列 8 個（entry 共有・8 スレッド） |
|---|---|---|
| Vaporizer2 3.5.0 | OK | **segfault（STATUS_ACCESS_VIOLATION）** |
| Dexed 1.0.1 | OK | OK |
| Surge XT 1.3.4 | OK | OK |

- **2 スレッドでも落ちる。3/3 で再現**（間欠ではない）
- entry を 1 つに共有しても落ちるので `PluginEntry::load` の競合ではなく**生成そのもの**
- 対策は [0013](0013-serial-instantiation.md)（プラグイン別に生成を直列化）。
  **上の 60 回はその対策が入った状態の数字**で、外すと同じ経路が落ちる

## ただし、これは測定であって証明ではない

- 16 instance の成功は **Windows / 48kHz / buffer 512** で、**Dexed v1.0.1（160 回）/
  Surge XT 1.3.4 / Vaporizer2 3.5.0（3 プラグイン 60 回）**の組み合わせで測った結果にすぎない
- **プラグインのバージョンが上がったら取り直すこと。** plugin / version 別の回帰対象として扱う
- **対応プラグインを増やしたら、まず `examples/parallel_instance_creation.rs` を通すこと。**
  Vaporizer2 がそこで落ちた
- **型では保証されない。** コンパイラは助けてくれない

## 未解決の観測

`dexed_survives_parallel_creation_handoff_and_playback_of_sixteen_instances` の行を出した直後に
**テストプロセスごと異常終了したことが 1 度だけある**（assert 失敗の出力なし）。
同じコマンドを続けて 7 回走らせたが再現せず、**原因未特定**。port / SHM の取り合いを疑っている。

## 壊れたら気づく場所

- `player::instances::tests::swapping::swapping_plugins_under_a_running_render_loop_survives_many_cycles`
  — 演奏中の跨ぎ差し替え（2 プラグイン）
- `player::instances::tests::swapping::swapping_across_three_plugins_under_a_running_render_loop_survives_many_cycles`
  — 同（3 プラグイン。**背景スレッドの生成と worker の生成が重なっても落ちないこと**まで見る）
- `core-lib/src/render/tests/dexed.rs::dexed_survives_parallel_creation_handoff_and_playback_of_sixteen_instances`
  — 起動時の並列生成
- `cargo run --release --example parallel_instance_creation -- "<CLAP>" 8` の**終了コード**
  — 新しいプラグインが並列生成に耐えるか（0 / 139）
