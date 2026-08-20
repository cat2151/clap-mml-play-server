# ADR 0009: unsafe thread handoff は測定で受け入れている（証明ではない）

- 状態: 記録（2026-08-20 実測 / 判定: 白）
- 関連: [0008](0008-spare-instance-pool.md) / [0012](0012-measured-baselines.md)

## 何を踏んでいるか

`core-lib/src/render/parallel.rs` は CLAP の
**「`init()` したスレッドと `clap_plugin_state.load` を呼ぶスレッドは同じ main thread」**
という規約を**意図的に破っている**。`!Send` な instance を unsafe に別スレッドへ移送する
（`RendererHandoff`、旧 `SendRenderer`）。

**予備インスタンスプール（[0008](0008-spare-instance-pool.md)）は、これを起動時 1 回ではなく
演奏中ずっと踏む。** これが混在計画で唯一の未知だった。

## 実測の結果: 白

**160 回の跨ぎ差し替えを演奏しながら通して異常なし。** 予備プールは成立する。

## ただし、これは測定であって証明ではない

- 16 instance の成功は **Windows / Dexed v1.0.1 / 48kHz / buffer 512 の 1 組み合わせ**の結果にすぎない
- **プラグインのバージョンが上がったら取り直すこと。** plugin / version 別の回帰対象として扱う
- **型では保証されない。** コンパイラは助けてくれない

## 未解決の観測

`dexed_survives_parallel_creation_handoff_and_playback_of_sixteen_instances` の行を出した直後に
**テストプロセスごと異常終了したことが 1 度だけある**（assert 失敗の出力なし）。
同じコマンドを続けて 7 回走らせたが再現せず、**原因未特定**。port / SHM の取り合いを疑っている。

## 壊れたら気づく場所

- `player::instances::tests::swapping_plugins_under_a_running_render_loop_survives_many_cycles`
  — 演奏中の跨ぎ差し替え
- `core-lib/src/render/tests.rs::dexed_survives_parallel_creation_handoff_and_playback_of_sixteen_instances`
  — 起動時の並列生成
