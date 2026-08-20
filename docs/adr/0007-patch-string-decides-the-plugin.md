# ADR 0007: patch 文字列でプラグインを判別する（IPC / SHM は無改修）

- 状態: 採用（2026-08-20）
- 関連: [0008](0008-spare-instance-pool.md) /
  clap-mml-render-tui `docs/adr/0001-patch-string-decides-the-plugin.md`（決定の本体）

## 決定

「patch 文字列 → どちらのプラグインで開くか」は **`.syx` コンポーネントを含むか**で決める。

```rust
// core-lib/src/dx7/patch_path.rs
pub fn is_cartridge_patch_path(patch: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(has_syx_extension)
}
```

`core-lib/src/render.rs` のロード経路は**すでにこれで分岐しており `plugin_id` を見ていない**。
つまりこの関数は混在対応の前から存在していた。

## もともと混在対応済みだった箇所

- **patch 一覧は `.fxp` と `.syx` を同じ walk で拾う**（`core-lib/src/patch_list.rs` の
  `collect_patches()` → `visit_dir()`）。`patches_dirs` に Surge の 2 本と Dexed の cartridge dir を
  並べれば、**混在カタログはそのまま得られる**
- **capability の差は instance 単位で吸収済み**（[0002](0002-capability-driven-ports-and-dialects.md)）
- **`MAX_INSTANCE_COUNT = 32` を増やしても SHM レイアウトは変わらない**
  （`realtime-ipc/src/lib.rs` の doc コメント）

## 帰結: IPC に足す情報は 0

初版の設計資料が最大の障害として挙げていた 2 点は消える:

- 「SHM v8 の `CommandSlot` / `SharedRing` に plugin 種別のフィールドが 1 bit も無い」
- 「`FastMidiCommand::PreparePatch { request_id, instance_id, patch, probe }` は
  patch のパス文字列しか送れない」

patch 文字列そのものがプラグインを決めるなら、サーバーは受け取った patch を
`is_cartridge_patch_path()` に通すだけでよい。したがって:

- **SHM の VERSION 上げ（v8 → v9）は不要**
- **`"CLAP preset"` JSON wire 形式は不要**
- **`PresetRef` tagged enum は不要**

## 種別の知識は core-lib にある

`core-lib/src/plugin_catalog.rs`（旧 `player/instances/kind.rs`）:
`PluginKind` / `plugin_kinds`（config → 種別一覧）/ `kind_for_patch`（patch 文字列 → 種別）/
`PatchBases`（形ごとの相対パス基点）。

**realtime play server だけでなく render server も使う**ので core-lib に置いてある。
`core-lib` が `cmrt-server-config` に依存する形になった（依存の向きは
clap-mml-render-tui `docs/adr/0010-two-repo-layout.md` と整合する）。

## render server の動きが変わった点（意図的）

**render server は、インストール済みプロファイルぶんの entry を全部ロードする。**

- `active_plugin = "Dexed"` でも **`.fxp` の音色を指した MML が鳴る**（以前は失敗していた）。
  保存済みの notepad / DAW が Surge 時代の音色を指しているとき、これが効く
- worker 起動時の `load_entry` が種別の数だけ増える（実測 40〜112ms/種別。
  [0012](0012-measured-baselines.md)）
- `SURGE_DATA_HOME` の判定が「既定が Surge か」→「載りうるものに Surge があるか」

## 罠

- **`load_cartridge_patch()` には Stage 1 でプラグインの照合を足した**（`ensure_cartridge_capable`）。
  Dexed 以外へ cartridge 形式を送ろうとするとエラーになる。
  **逆方向（Dexed へ `.fxp` の state load）には照合を入れていない。**
  そちらはプラグインが state load を失敗させるので、黙って無視されることがないため
- **照合が無いと「静かに間違う」。** Surge のインスタンスへ DX7 の SysEx を送ると、
  Surge は理解できない 163 byte を**黙って無視する。エラーにならない**
- 判別材料は patch 文字列の形だけなので、**同じ形を扱うプラグインが 2 つ載ると区別できない**
