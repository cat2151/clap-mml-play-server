# ADR 0018: 音色ロードの下準備で `process()` を空回ししてよいかは、プラグインの契約で決める

- 状態: 採用（2026-09-03）
- 関連: [0002](0002-capability-driven-ports-and-dialects.md) /
  [0019](0019-cache-player-slot-headroom.md) /
  clap-mml-render-tui `docs/adr/0012-live-clock-drift-is-absorbed-not-eliminated.md`

## 背景（実測で確定した不具合）

DAW の live 演奏（cache-player でキャッシュ WAV を鳴らす経路）で、**小節の頭の直後に
音が 53.3ms 飛ぶ**。飛んだあとは小節の最後まで 53ms 早いまま鳴り、次の小節の頭で正しい
位置へ戻る。副作用として**小節の終わりの 40〜95ms が鳴らずに終わる**（飛んだぶん早く
WAV の終端に着くため）。

出た音を録って打点を 1 つずつ拾うまで、この不具合は 1 つも観測できなかった。
サーバーの帳簿はすべて正常だった（`late=0` / `underrun_level=0` / 予約位置の間隔は
サンプル完全一致）。**録った波形でだけ見える不具合だった。**

| その小節の打点 | 予約位置からのずれ |
|---|---|
| 1 個目（小節の頭） | +240 frames … 全小節で同じ。これが正しい基準 |
| 2 個目（+133ms） | 潰れる |
| 3 個目以降 | **−2320 frames（−48.3ms）で完全に一定** |

**2560 = 512 × 5**（オーディオブロック 5 個ぶん）ちょうど。

## 原因

`prepare_patch`（`realtime-play-server/src/player/worker/bank/state.rs`）は、音色ロードの
前後で `process()` を空回ししていた:

| 何を | 何ブロック | なぜあったか |
|---|---|---|
| `renderer.reset()` | **1**（`core-lib/src/render/silence.rs` が all sound off を 1 ブロック流す） | 前の音色の voice を確実に切るため |
| `settle_patch()` | **4**（`PATCH_SETTLE_BLOCKS`） | Dexed は state load だけでは反映されないため |

**`process()` を呼べば、プラグインの中の時間はそのぶん進む。** 出力は捨てられるが、
鳴っている voice の再生位置は進む。

Surge XT や Dexed では害が出ない。**差し替えの前に voice を切っている**から、進む音が
そもそも無い。cache-player は逆に「**鳴っている音をスロットの差し替えで切らない**」
（voice は自分が握った `Arc<CacheBuffer>` を鳴らし続ける）ので、DAW の先読み
（小節 N を鳴らしている最中に同じ instance の別スロットへ小節 N+1 を載せる）のたびに
5 ブロックぶん音が飛んでいた。

## 決定

**差し替えの前後の下準備までを、`RealtimeRenderer::switch_patch` という 1 つの入口に閉じた。**

```rust
pub fn switch_patch(&mut self, patch: Option<&str>, reset_before: bool, settle_blocks: usize)
```

- `keeps_voices_across_patch_load(plugin_id)` が真のプラグインでは
  **`reset()` も settle の空回しも 1 ブロックも回さない**
- 真になるのは `CACHE_PLAYER_PLUGIN_ID` だけ
- `prepare_patch` / `probe_patch` は `reset` → `set_patch` → `settle` を自分で組むのをやめ、
  `switch_patch` を呼ぶだけになった。`settle_patch()` は削除（中身は core-lib へ）

cache-player の state load は `load()` がスロットへ `Arc` を差した時点で完了しているので、
**反映のための空回しは元から要らない**（反映は次の `process()` の `refresh_slots`）。

## なぜ呼び出し側で分岐しないのか

呼び出し側ごとに手順を組み立てていると、この副作用を 1 か所で止められない。
「voice を切ってから差し替える」を前提にした下準備は、その前提を持たないプラグインが
1 つ増えるだけで壊れる。**プラグインの契約として問うのが正しい形。**

## 判定（決定的なテスト・録音に頼らない）

`core-lib/src/render/tests/cache_player.rs` の
`a_prefetch_while_sounding_does_not_advance_the_playing_voice`。

**値がそのままフレーム番号になる ramp WAV**（`value = frame / 48000`）を鳴らし、
2 ブロック鳴らしたところで先読みを 1 回入れて、次のブロックの先頭が
**フレーム 1024 のままか**を見るだけ。`.clap` ファイルもユーザーのキャッシュも要らないので
`cargo test` で毎回走る。

```text
$ cargo test -p cmrt-core --lib render::tests::cache_player     # 直したあと
test ... a_prefetch_while_sounding_does_not_advance_the_playing_voice ... ok

$ # 壊して（switch_patch の may_spin を true 固定に）もう一度
assertion `left == right` failed: 先読みで再生位置が 2560 フレーム飛んだ
  left: 3584
 right: 1024
```

**この 2560 は実録音から測った 2560 と同じ数。** ログでも理屈でもなく、
**壊れ方の量が一致した**ので原因の特定はここで確定した。

## なぜ決定的なテストが要ったか

実録音では **3 回に 2 回しか壊れなかった**（飛びが 1 つ前の小節の終わりに当たった回は
無傷に見える）。同じコード・同じ素材で緑にも赤にもなるので、録音だけを判定にすると
「直った」を誤って宣言する。**決定的なテストを主、録音を裏取りに使うこと。**

## 実録音での裏取り（release / 3 回）

`python scripts/capture_daw_live_mix.py --only-row 6 --loop-measures 4 --measures 8 --tracks 9`
（TUI 側 repo）

| | 直す前 | 直したあと（3 回とも） |
|---|---|---|
| 小節ごとの `lag` | meas1 だけ +240、以降すべて −2304 | **全小節 +240**（spread 0） |
| 小節ごとの `corr` | 0.42〜0.73 | **1.00** |
| 小節の中の `drift_spread` | 0 or **2560**（3 回に 2 回壊れる） | **0** |
| **切れている窓**（余韻） | 各小節 39〜94ms（**無傷に見えた回でも**） | **0** |

## 壊れたら気づく場所

| テスト | 落ちたら |
|---|---|
| `render::tests::cache_player::a_prefetch_while_sounding_does_not_advance_the_playing_voice` | 空回しが戻った（鳴っている音が先読みのたびに飛ぶ） |
| `render::patch_switch::tests` | 「鳴っている音を切らない」が cache-player 以外へ広がった／狭まった |
