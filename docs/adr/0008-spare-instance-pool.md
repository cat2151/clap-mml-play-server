# ADR 0008: 予備インスタンスプール（論理スロットと物理インスタンスの分離）

- 状態: 採用
- 関連: [0003](0003-dexed-program-change-guard.md) / [0007](0007-patch-string-decides-the-plugin.md) /
  [0009](0009-unsafe-thread-handoff.md) / [0012](0012-measured-baselines.md)

## 決定

**論理 `instance_id` と物理 CLAP インスタンスを分離する。**

- `instance_id` は今のまま**論理スロット**として残す
  （TUI 側の対応は `instance_id = bank * track_count + 行番号`）
- サーバーが「論理 → 物理」の対応表と、**プラグインごとの予備プール**を持つ
- `PreparePatch{instance_id, patch}` を受けたら:
  1. patch 文字列から必要なプラグインを決める（[0007](0007-patch-string-decides-the-plugin.md)）
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

**`prepare_slot_for_patch` はレンダリングと同じ worker スレッドで走る。**
予備が尽きると `take_spare` がそこでブロックし、Surge 1 個ぶん（約 490ms）レンダリングが止まる。
出力リングは grid sequencer 入場時に約 21ms（TUI の `INITIAL_BUFFER_MULTIPLIER = 2`）しか
ないので、**underrun は確定**する。音が途切れ、`AdaptiveBuffer` の梯子が上がって遅延が増え、
先読みが 1 小節に間に合わずその周は音色が変わらない。

そこで `spare_target()` を **「スロット数ぶん、上限 `MAX_DEFAULT_SPARE_TARGET = 8`」**にし、
`LiveInstances::new` が起動時に目標数ぶんまとめて発注する。
**待ちは演奏中から起動直後のアイドルへ移る**（総生成コストは変わらない。7 行が同時に Surge へ
飛ぶ初回の待ちが約 3 秒 → 0ms、前払い約 3.9 秒はアイドル中の背景）。

`CMRT_SPARE_INSTANCES` で目標数を上書きできる（`1` で前払いをやめた挙動、`0` で予備プールごと停止）。
前払い方式を選んだ決め手は、**効かなかったときのロールバックがこの環境変数 1 つで済む**こと。

## 採らなかった案:「補充を先読み経路へ移す」

**1ms も縮まらない。** 先読み経路（1 ステップ 1 件の `preload`）は既に 1 小節ぶん先行しており、
発注を前倒ししても**背景生成スレッドが 1 本で 1 個ずつ直列**なので、
N 個ぶんの合計待ち時間は変わらない。**builder が律速。**
後続の「背景生成の並列化」「待ちを worker の外へ出す」は前払いの上へ足せる
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
  1 件ずつ積むと**アイドル中に前払いが 1 個で止まる**。発注は worker の都合から切り離して一括で積むこと
- **`prepare_slot_for_patch` で予備が尽きる経路を増やさないこと**（上記の underrun）
- **auto gain の RMS 履歴**は物理インスタンスごとに溜まる。差し替えると別プラグインの履歴を
  引き継ぐが、自己補正するので実害は小さい
- **「偶数 track / 奇数 track でハードコード」は壊れる。**
  `instance_id = bank * track_count + 行番号` なので、偶奇は「行」ではなく
  **bank をまたいだ別の行**を分けてしまう。`track_count` が奇数のとき小節境界の bank 切替で
  同じ行のプラグインが入れ替わる。分けるなら `instance_id % track_count`
- 予備プールの「先頭 kind が既定プラグイン（音色無指定の行が鳴るもの。
  [0017](0017-fixed-surge-primary-plugin.md) で Surge XT 固定）」という内部契約は変えない
- worker は entry を保持しない（保持するのは背景生成スレッドだけ。CLAP インスタンスが
  entry の clone を持つので足元は崩れない）

## 壊れたら気づく場所

- `player::instances::tests::a_prepaid_spares_are_all_ordered_up_front`
- `player::instances::tests::swapping_plugins_under_a_running_render_loop_survives_many_cycles`
