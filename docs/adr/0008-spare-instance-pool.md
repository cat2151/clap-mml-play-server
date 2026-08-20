# ADR 0008: 予備インスタンスプール（論理スロットと物理インスタンスの分離）

- 状態: 採用（2026-08-20）
- 関連: [0003](0003-dexed-program-change-guard.md) / [0007](0007-patch-string-decides-the-plugin.md) /
  [0009](0009-unsafe-thread-handoff.md) / [0012](0012-measured-baselines.md)

## 決定

**論理 `instance_id` と物理 CLAP インスタンスを分離する。**

- `instance_id` は今のまま**論理スロット**として残す
  （TUI 側の対応は `instance_id = bank * track_count + 行番号`）
- サーバーが「論理 → 物理」の対応表と、**プラグインごとの予備プール**を持つ
- `PreparePatch{instance_id, patch}` を受けたら:
  1. `is_cartridge_patch_path(patch)` で必要なプラグインを決める
  2. いまその論理スロットに紐づく物理インスタンスのプラグインと同じなら、従来どおり patch をロード
  3. 違うなら、**今の物理を（音を止めてから）プールへ返し、必要なプラグインの予備を取り出して
     論理スロットへ結び直し**、patch をロードする
- 予備が尽きたら背景で確保する

## なぜ「作り直し」ではなく「プール」か

**TUI が完全に無改修で済む。** MIDI ルーティング・instance ごとの gain・auto gain の配列
（`realtime-ipc` の `auto_gain_db_bits: [AtomicU32; MAX_INSTANCE_COUNT]`）は
すべて論理 id で引かれているので、物理が入れ替わっても何も変わらない。

**作り直し方式（不採用）との差**: 都度 `RealtimeRenderer::new_with_timing()` を呼ぶ方式だと、
wheel を 1 目盛り回すたびに Surge で 200〜360ms 待たされる。
wheel は連続で回すものなので実用に耐えない。プールなら**取り出しは即時**で、
生成コストは背景へ逃げる。

## 深さは「スロット数ぶん、上限 8」を起動直後に前払いする

当初は「予備 1 つずつ」だったが、実測で本当の実害が見つかった。

**`prepare_slot_for_patch` はレンダリングと同じ worker スレッドで走る。**
予備が尽きると `take_spare` がそこでブロックし、Surge 1 個ぶん（約 490ms）レンダリングが止まる。
出力リングは grid sequencer 入場時 `INITIAL_BUFFER_MULTIPLIER = 2` = 512×2 = **約 21ms** しか
ないので、**underrun は確定**する。UX としては

1. 音が途切れる
2. `AdaptiveBuffer` の梯子が上がって遅延が増える
3. 先読みが 1 小節（BPM 130 で 1.85 秒）に間に合わず、その周は音色が変わらない

**入れた形**: `spare_target()` を「固定 1」から
**「スロット数ぶん、上限 `MAX_DEFAULT_SPARE_TARGET = 8`」**へ。
`LiveInstances::new` が起動時に目標数ぶんまとめて発注するので、
**待ちは演奏中から起動直後のアイドルへ移る**（総生成コストは変わらない）。

| 見るもの | 前 | 後 |
|---|---|---|
| 7 行が同時に Surge へ飛ぶ初回 | 2,969ms | **0ms**（前払い 3,851ms はアイドル中の背景） |
| 実機の起動（`active_plugin = 'Dexed'` / release） | listen 117ms | listen 117ms（変わらず）。予備 8 個が since_boot 1,334ms で揃う |
| 最悪ケース 32 スロット | working set 793MB | 807MB（+1 instance ぶん） |

## 採らなかった案:「補充を先読み経路へ移す」

**1ms も縮まらない。** 先読み経路（`begin_cycle_swap` → 1 ステップ 1 件の `preload`）は既に
1 小節ぶん先行しており、発注を前倒ししても**背景生成スレッドが 1 本で 1 個ずつ直列**なので、
N 個ぶんの合計待ち時間は変わらない。抽選の瞬間に次サイクル全件を伝えても同じ。**builder が律速。**

前払い方式を選んだ決め手は、**効かなかったときのロールバックが環境変数 1 つで済む**こと。
後続の「背景生成の並列化」「待ちを worker の外へ出す」もこの上へ足せる
（逆に後者を先にやると TUI 側へ再送プロトコルが入って戻しにくい）。

## エビクションは要らない

物理インスタンス数の上限は `スロット数 + 予備の目標数` で**構造的に決まる**。
背景生成は 1 スレッドのまま（並列にしても取り合うだけ）。
実測: 32 Surge + 32 Dexed = working set 793 MB。

## 忘れると壊れる点

- **プールへ返す前に必ず all-notes-off を送る。** 返した物理インスタンスが鳴りっぱなしになる
  （`silence_all_notes()`）
- **`set_patch(None)` は state load なので Dexed の 2 秒 guard を armed にする**
  （[0003](0003-dexed-program-change-guard.md)）。プールへ返すときに初期化すると、
  次に取り出した直後の program 変更が捨てられうる
- **差し替えは worker スレッドで行う。** コマンドの適用と同じ場所なので、
  オーディオブロックの境界で自然に直列化される
- **予備の発注は 1 件ずつ積んではいけない。** 受け取り（`collect_ready`）は worker の
  ループからしか走らず、コマンド待ちの `wait_for_command()` でブロックしている間は動かない。
  1 件ずつ積むと**アイドル中に前払いが 1 個で止まる**（実機ログで `spare_built` が 1 行しか
  出なかった）。発注は worker の都合から切り離して一括で積むこと
- **`prepare_slot_for_patch` で予備が尽きる経路を増やさないこと**（上記の underrun）
- **auto gain の RMS 履歴**は物理インスタンスごとに溜まる。差し替えると別プラグインの履歴を
  引き継ぐが、自己補正するので実害は小さい
- **極小案として「偶数 track / 奇数 track でハードコード」を試すなら、偶奇では壊れる。**
  `instance_id = bank * track_count + 行番号` なので、偶奇は「行」ではなく
  **bank をまたいだ別の行**を分けてしまう。`track_count` が奇数（1 / 3 / 7）のとき
  bank 0 の行 1 と bank 1 の行 1 が別プラグインになり、小節境界の bank 切替で
  同じ行のプラグインが入れ替わる。分けるなら `instance_id % track_count`

## 実装の地図

| 場所 | 役割 |
|---|---|
| `realtime-play-server/src/player/instances.rs` | `LiveInstances`。論理スロット → 物理インスタンスの対応表、予備の袋、差し替え（`prepare_slot_for_patch`） |
| `realtime-play-server/src/player/instances/builder.rs` | 背景生成スレッド。entry は要求されて初めてロードする |
| `core-lib/src/plugin_catalog.rs` | `PluginKind` / `plugin_kinds` / `kind_for_patch` / `PatchBases` |
| `core-lib/src/render/parallel.rs` | `RendererHandoff`（旧 `SendRenderer`）。`create_renderers_parallel` は index ごとに別の `(cfg, entry)` を取る `RendererSpec` 方式 |
| `core-lib/src/render/cartridge_patch.rs` | プラグインの照合（`ensure_cartridge_capable`） |
| `server-config/src/plugin_profile.rs` | `PatchForm` / `patch_form_of` / `merged_plugin_profiles` / `installed_plugin_profiles` |
| `realtime-play-server/src/player/live.rs` | `resolve_live_patch` が形ごとの基点を使う |

`active_plugin` は「既定プラグイン（＝音色無指定の行が鳴るプラグイン）」の意味で残している。
worker は entry を保持しない（保持するのは背景生成スレッドだけ。CLAP インスタンスが
entry の clone を持つので足元は崩れない）。

## 環境変数とログ

`CMRT_SPARE_INSTANCES` — プラグインごとの予備の目標数。
既定は「スロット数ぶん、上限 8」。`1` で前払いをやめた従来の挙動、`0` で予備プールごと停止。

観測できるログ（stderr）:

- `phase=spare_built plugin=... ms=... result=ok` — 背景生成 1 個ぶん（生成スレッドが出す）
- `phase=spare_ready plugin=... spares=N physical=M` — 袋へ取り込んだ（worker が出す）
- `phase=instance_swap slot=N from=A to=B ms=... spares=N physical=M` — 差し替え

## 壊れたら気づく場所

- `player::instances::tests::a_prepaid_spares_are_all_ordered_up_front`
- `player::instances::tests::swapping_plugins_under_a_running_render_loop_survives_many_cycles`
