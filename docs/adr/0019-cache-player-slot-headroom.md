# ADR 0019: cache-player のスロットは 4 本（クロックの先行を吸収するための余裕）

- 状態: 採用
- 関連: [0018](0018-patch-load-must-not-spin-the-plugin.md) /
  clap-mml-render-tui `docs/adr/0012-live-clock-drift-is-absorbed-not-eliminated.md`

## 背景

DAW の live 演奏で **予約した小節とは違う小節が鳴る**ことがあった。録った live mix と
各キャッシュ WAV の整合フィルタで見ると、meas1 の位置で meas3、meas2 の位置で meas4 が鳴っていた。
meas1↔meas3 / meas2↔meas4 は**同じスロットを共有する組**（当時 `SLOT_COUNT = 2`）で、
サーバーログの `cmrt-live-patch: event=apply … clock=` が決定打: **上書きは note on が発火する
16891 フレーム（352ms）前**に起きていた。

原因は演奏ループ（TUI 側）がサーバーのサンプルクロックより**先行**していたこと。
サーバーは state load のあいだ render を回さないが、演奏ループは実時間から外挿するので、
止まっていたぶんがそのまま先行になる（実測 2.7 秒＝BPM113 で 1.3 小節）。
**先行の根本原因を絶つ案は見送った**（clap-mml-render-tui `docs/adr/0012-live-clock-drift-is-absorbed-not-eliminated.md`）ので、
**先行しても音が壊れないほうで受ける。**

## 決定

`SLOT_COUNT` を **2 → 4**。

DAW の先読みは 1 小節先まで。小節 index `N` はスロット `N % SLOT_COUNT` へ載り、
その小節の note on は `60 + (N % SLOT_COUNT)`。演奏ループが `D` 小節ぶん先行していると、
スロットへ書く瞬間にそこに居る小節の note on がまだ発火していないことがある。
**吸収できる先行はおよそ `SLOT_COUNT - 1` 小節。**

`60 % 4 == 0` なので `slot_for_note(60 + s) == s` は保たれる（note 60〜63）。
patch 文字列の綴りも SHM のレイアウトも変わらないので、**プロトコルの版は上がらない。**

## 3 案のうちこれを採った理由

上書きできないと分かったときにどうするか、で 3 案あった:

| 案 | 内容 | 採らなかった理由 |
|---|---|---|
| 1 | 諦める（その小節の先読みを捨てる） | 音は正しいが**先読みが外れる**（小節の頭が state load ぶん無音になる。先読みはそれを消すために入れたもの） |
| 2 | 待つ（note on が発火するまでロードを遅らせる） | **演奏スレッドが止まる**。止まっているあいだにさらに先行が積む |
| 3 | **スロットを増やす** | 採用。**値段はメモリだけ** |

## 値段（実測）

キャッシュ WAV は 4 秒ステレオ f32 = 1.54MB/本。release の play server（`--instances 8`、
DAW の 7 行を 7 instance で鳴らす）で演奏中のピーク working set は 2 → 4 本で **+24.2MB**
（理論値 7 × 2 × 1.54MB ≒ 21MB とほぼ一致）。
**最悪でも 16 instance × 4 本 × 1.54MB = 98MB**（2 本なら 49MB）。

RT スレッドは `Arc` の clone しかしない（確保も解放もしない）ので、
**増やしても `process` の重さは変わらない。**

## 効き目（同じ先行を注いだ A/B）

`MeasureTimeline` の原点を 2 小節ぶん過去へずらす細工（＝演奏ループがサーバーより
2 小節先行している状態そのもの）を入れて、TUI 側の `python scripts/capture_daw_live_mix.py`
を release サーバーで走らせた。`SLOT_COUNT = 2` ではスロット踏み潰し 10 件で小節ごとの `corr`
が 0.40〜0.45（違う小節が鳴っている）、`4` では踏み潰し 0 件で `corr` 0.70〜0.74
（先行なしのときと同じ）。先行が無いときの余裕も 1.02 小節 → 3.03 小節へちょうど 3 倍。

## 余裕はループ長にも依る（残っている穴）

スロットは `小節 index % SLOT_COUNT` で決まるので、**ループ長が `SLOT_COUNT` の倍数で
ないと、ループの折り返しで同じスロットへの書き込みが間を空けずに続く。**
TUI 側のテスト（`daw/src/playback/live_cache/tests/slot_headroom.rs`）で数を固定してある:

```text
余裕 = 無制限                        （ループ長 <= SLOT_COUNT）
余裕 = SLOT_COUNT - 1                （ループ長が SLOT_COUNT の倍数）
余裕 = (ループ長 % SLOT_COUNT) - 1   （それ以外）
```

- **1〜4 小節のループ（実演奏はここ）は、先行が何秒あっても壊れない。**
  小節とスロットが 1 対 1 になり、差し替えても中身が同じ小節だから
- **`ループ長 % SLOT_COUNT == 1` のときは余裕 0。** 4 スロットなら 5・9・13 小節のループ
  （2 スロットなら 3 以上の奇数すべて）

この穴は**本数を増やしても消えない**。消すにはスロットの選び方を「小節 index」から
「演奏した小節の通し番号」へ変える必要がある。今回は入れていない
（clap-mml-render-tui `docs/adr/0012` の「残っているリスク」）。

## 壊れたら気づく場所

| テスト | 落ちたら |
|---|---|
| `cache-player` `slots::tests::the_slot_count_leaves_room_for_a_clock_drift_of_three_measures` | 本数が減った（`const { assert! }` なので**コンパイルが通らなくなる**） |
| `cache-player` `slots::tests::note_numbers_wrap_around_the_slots` | note 60 とスロット 0 の対応が崩れた（`SLOT_COUNT` が 60 を割らない値になった） |
| TUI `live_cache::tests::slot_headroom::a_clock_drift_of_three_measures_still_plays_the_measure_that_was_reserved` | 余裕が 3 小節を切った（実測の先行 1.3 小節に対する保険が消える） |
| TUI `live_cache::tests::slot_headroom::the_headroom_shrinks_when_the_loop_length_is_not_a_multiple_of_the_slot_count` | 余裕の出かたが変わった（スロットの選び方を変えたなら、この表を書き直すこと） |
