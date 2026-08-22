# ADR 0013: 並列生成に耐えないプラグインだけ、instance 生成を直列化する

- 状態: 採用（2026-08-22）
- 関連: [0008](0008-spare-instance-pool.md) / [0009](0009-unsafe-thread-handoff.md) /
  [0012](0012-measured-baselines.md)

## 背景

instance をスレッド並列に作る場所が 4 つある:

- 起動時の `create_renderers_parallel`（8 スレッド）
- 予備インスタンスプールの背景スレッド（[0008](0008-spare-instance-pool.md)）
- capability probe
- オフライン render server の worker

CLAP の規約では instance 生成は main thread 限定なので、これはもともと賭け
（[0009](0009-unsafe-thread-handoff.md)）で、Surge XT と Dexed では当たっていた。

**Vaporizer2 3.5.0 で外れた。2 スレッドでも `STATUS_ACCESS_VIOLATION` でプロセスごと落ちる**
（3/3 で再現。entry を共有しなくても落ちるので `PluginEntry::load` の競合ではなく
instance 生成そのもの）。直列なら 8 個でも 16 個でも通る。

## 決定

`core-lib/src/render/serial_instantiation.rs` に**プロセス共通の `RwLock` を 1 本**置き、

| プラグイン | 取るもの | 意味 |
|---|---|---|
| 並列に作れる（Surge XT / Dexed / 未知） | **read** | 今までどおり並列に作る |
| 作れない（Vaporizer2） | **write** | 自分どうしとも、他プラグインとも重ならない |

`InstantiationPermit::acquire(plugin_id)` を
**`create_plugin_instance_without_patch()`（`core-lib/src/render/instance.rs`）**で取る。

## なぜ `Mutex` ではなく `RwLock` か

read 同士は競合しないので、**Surge XT だけ / Dexed だけの構成の速度が 1 ミリ秒も変わらない**。
`Mutex` にすると Surge 8 個の並列生成が 1086ms → 4275ms（直列）へ落ちる。

番人テスト `two_parallel_safe_plugins_hold_the_lock_at_the_same_time` がこれを固定している。
**ここが exclusive になると、Vaporizer2 を入れていないユーザーの起動が 4 倍遅くなる。**

## なぜ「他プラグインとも重ならない」まで広げるか

Vaporizer2 の生成と Surge の生成が同時に走って落ちないことを**別途実測していない**。
安い側（read を 1 回取るだけ）へ倒してある。

## なぜ instance 生成の入口 1 か所に置くか

`create_plugin_instance_without_patch()` は **instance を作る唯一の入口**。
呼び出し側（起動時の並列生成・予備プールの背景スレッド・probe・render server の worker）が
今後増えても**掛け忘れない**。呼び出し側に置くと、増えた 5 つめが黙って落ちる。

## 何を守り、何を守らないか

守るのは **`create_plugin()` + `init()` の区間だけ。**

Vaporizer2 のコンストラクタはプリセット走査（`reloadPresetArray`）を**非同期スレッドへ投げる**ので、
生成から戻ったあとも裏で走っている。それを待たないのは、**「直列生成」と呼んで通ることを
確かめてある形がまさにそれ**（次の生成が前の走査と重なる）だから。
待つ設計にすると、確かめていない形へ勝手に変わる。

## `CMRT_SERIAL_INSTANTIATION` は A/B 専用

`off` / `0` / `false` を渡すとロックを取らなくなる。**未設定は必ず有効側。**

これがあるので「落ちなくなったのは直列化のおかげ」を機械で示せる:

| | 直列 8 個 | 並列 8 個（8 スレッド） | 終了コード |
|---|---|---|---|
| 直列化を入れる前 | ok | **segfault** | **139** |
| 入れたあと | ok | ok（1 個ずつ 110ms 刻みで完成する） | **0** |
| 入れたあと + `CMRT_SERIAL_INSTANTIATION=off` | ok | **segfault**（再現する） | **139** |

```
cargo run --release --example parallel_instance_creation -- "<CLAP のパス>" 8
```

**対応プラグインを増やすときは必ずここを通すこと。** 終了コードで判定できる。

## 判定材料は descriptor の本物の ID

`plugin_requires_serial_instantiation(plugin_id)` は **CLAP descriptor から読んだ ID**で決める。
config の `plugin_id`（ユーザーが書く推測値）ではない。省略されている config でも効く。

## コスト（実測 / release / warm）

Vaporizer2 の生成は 1 個 97〜107ms。直列 8 個で 845ms、並列 8 個でも 895ms
（直列化されるので同じ）。**Surge XT の 1/5 の時間**なので、直列化しても Surge より速い。
数字は [0012](0012-measured-baselines.md)。

## 壊れたら気づく場所

| テスト（`core-lib/src/render/serial_instantiation/tests.rs`） | 落ちたら |
|---|---|
| `only_vaporizer2_is_built_one_at_a_time` | 直列化の対象が広がっている（起動が遅くなる） |
| `two_parallel_safe_plugins_hold_the_lock_at_the_same_time` | Surge / Dexed 単独構成の並列生成が 1 本へ落ちた |
| `a_serial_plugin_takes_the_exclusive_side` | Vaporizer2 が read 側へ落ちた（並列生成で segfault が戻る） |
| `the_escape_hatch_is_opt_in_and_case_insensitive` | **未設定が無効側へ倒れた**（何も設定していない実運用が落ちる） |
