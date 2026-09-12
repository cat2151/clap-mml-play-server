# ADR 0007: patch 文字列でプラグインを判別する（IPC / SHM は無改修）

- 状態: 採用
- 関連: [0008](0008-spare-instance-pool.md) / [0014](0014-vvp-as-clap-state.md) /
  clap-mml-render-tui `docs/adr/0001-patch-string-decides-the-plugin.md`（決定の本体）

## 決定

「patch 文字列 → どのプラグインで開くか」は **patch 文字列に現れる拡張子**で決める。

```rust
// core-lib/src/dx7/patch_path.rs
pub fn is_cartridge_patch_path(patch: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(has_syx_extension)
}
// core-lib/src/vvp.rs
pub fn is_vvp_patch_path(patch: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(has_vvp_extension)
}
```

`core-lib/src/render.rs` のロード経路はこれで分岐し、`plugin_id` を見ない。

## `PatchForm` は 5 値

| 形 | 拡張子 | 単位 | ロードのしかた |
|---|---|---|---|
| `StateFile` | `.fxp` | 1 ファイル = 1 音色 | FXP の chunk を切り出して `clap.state` へ |
| `Cartridge` | `.syx` | 1 ファイル = 32 program | packed voice を single voice SysEx で送る（[0003](0003-dexed-program-change-guard.md)） |
| `Vvp` | `.vvp` | 1 ファイル = 1 音色 | XML に 9 バイト被せて `clap.state` へ（[0014](0014-vvp-as-clap-state.md)） |
| `FloePreset` | `.floe-preset` | 1 ファイル = 1 音色 | Floe 固有 extension へ |
| `Sfz` | `.sfz` | 1 ファイル = 1 音色 | ARIA program 解決 + vendor state（[0015](0015-sforzando-sfz-preset-load.md)） |

`Vvp` の単位は `StateFile` と同じだが、**別の形として数える**。一緒にすると
Surge XT と Vaporizer2 のどちらへ送るべき patch かが決まらず、片方の音色が
もう片方のインスタンスへ流れる。

**判別規則は `patch_form_of_path()`（`core-lib/src/audio_plugin.rs`）へ 1 本化してある。**
`kind_for_patch()` と `PatchBases::base_for()` が別々に書いていると、
**片方だけ直したときに「選ばれたプラグインと基点が食い違う」**という静かな間違いになる。
順序は cartridge → sfz → floe-preset → vvp → state_file（`StateFile` が「どれでもない」の受け皿）。

## 帰結: IPC に足す情報は 0

patch 文字列そのものがプラグインを決めるなら、サーバーは受け取った patch を
判別関数に通すだけでよい。したがって:

- **SHM に plugin 種別のフィールドは要らず、VERSION 上げも不要**
- **`"CLAP preset"` JSON wire 形式は不要**
- **`PresetRef` tagged enum は不要**

patch 一覧は拡張子を問わず同じ walk で拾う（`core-lib/src/patch_list.rs` の `collect_patches()`）ので、
`patches_dirs` に各プラグインの dir を並べれば混在カタログになる。
capability の差は instance 単位で吸収済み（[0002](0002-capability-driven-ports-and-dialects.md)）。
`MAX_INSTANCE_COUNT` を増やしても SHM レイアウトは変わらない（`realtime-ipc/src/lib.rs` の doc コメント）。

## 種別の知識は core-lib にある

`core-lib/src/plugin_catalog.rs`:
`PluginKind` / `plugin_kinds`（config → 種別一覧）/ `kind_for_patch`（patch 文字列 → 種別）/
`PatchBases`（形ごとの相対パス基点）。

**realtime play server だけでなく render server も使う**ので core-lib に置いてある。
`core-lib` が `cmrt-server-config` に依存する（依存の向きは
clap-mml-render-tui `docs/adr/0010-two-repo-layout.md` と整合する）。

## render server はインストール済みプロファイルぶんの entry を全部ロードする

- 既定プラグインが何であっても **`.fxp` の音色を指した MML が鳴る**。
  保存済みの notepad / DAW が別プラグインの音色を指しているとき、これが効く
- worker 起動時の `load_entry` が種別の数だけ増える（実測 40〜112ms/種別。
  [0012](0012-measured-baselines.md)）
- `SURGE_DATA_HOME` の判定は「載りうるものに Surge があるか」（[0010](0010-surge-data-home-and-plugin-identity.md)）

## 罠

- **`load_cartridge_patch()` はプラグインを照合する**（`ensure_cartridge_capable`）。
  Dexed 以外へ cartridge 形式を送ろうとするとエラーになる。
  **逆方向（Dexed へ `.fxp` の state load）には照合を入れていない。**
  そちらはプラグインが state load を失敗させるので、黙って無視されることがないため
- **照合が無いと「静かに間違う」。** Surge のインスタンスへ DX7 の SysEx を送ると、
  Surge は理解できない 163 byte を**黙って無視する。エラーにならない**
- **`.vvp` の照合（`ensure_vvp_capable`。`core-lib/src/render/vvp_patch.rs`）は省略できない**:
  `.vvp` の生 XML をそのまま `clap.state` へ渡すと、Surge は静かに間違うのではなく
  **プロセスごと落ちる**（STATUS_ACCESS_VIOLATION）。テストハーネスごと落ちるので
  テストにはできず、`core-lib/src/render/tests/vaporizer2.rs` にコメントだけ残してある
- **`load_vvp_state()` / `ensure_vvp_capable()` は自由関数**にしてある。
  config の `patch_path` に `.vvp` を書いた起動経路は `RealtimeRenderer` を
  組み立てる**前**（activate 前）にロードするので、メソッドでは呼べない
- 判別材料は patch 文字列の形だけなので、**同じ形を扱うプラグインが 2 つ載ると区別できない**。
  `.fxp` と `.vvp` は固有拡張子があったので永続 ID を変えずに解けたが、
  拡張子まで同じプラグインが 2 つ載る日には解けない（TUI 側 ADR 0001 の「未解決の論点」）
- **`.sfz` は Sforzando が無ければ既定プラグインへ fallback しない。** `kind_for_patch()` が
  Sforzando 不在をエラーにし、loader も plugin ID と ARIA program mapping を照合してから state load する
