# ADR 0022: Six Sines: `.sxsnp` は CLAP state として流し、factory は GitHub から取得する

- 状態: 採用
- 関連: [0006](0006-no-generic-clap-preset-api.md) / [0007](0007-patch-string-decides-the-plugin.md) /
  [0014](0014-vvp-as-clap-state.md) / [0018](0018-patch-load-must-not-spin-the-plugin.md)

## 決定

1. Six Sines の音色ファイル `.sxsnp` は、**バイト列を包み直さずにそのまま `clap.state` へ渡す**
   （`core-lib/src/render/six_sines_patch.rs`）
2. factory 音色は、descriptor version（例 `1.2.0.18ecb36`）の最後の `.` の後ろを commit hash とみなし、
   **同じ commit の `resources/factory_patches/` を GitHub から取得して**
   `config_app_dir()/vendor-patches/six-sines-factory/` に置く（`core-lib/src/six_sines_factory.rs`）。
   取得は tree API 1 回 + raw をファイルごと。取得するのは `cmrt build-patch-catalog-cache` のときだけで、
   記録済みの commit と同じなら通信しない
3. 置き場のパスに commit を入れない。display（置き場からの相対パス）が永続 ID なので、
   plugin を更新するたびに保存済みの MML が解決しなくなるのを避けるため

## 理由

- Six Sines の `stateSave` は patch の XML に NUL を 1 byte 足しただけを書き、`stateLoad` は NUL まで
  （無ければ末尾まで）を XML として読む（`sst-plugininfra` の `patch_base_clap_adapter.h`）。
  `.sxsnp` は同じ XML なので、[0014](0014-vvp-as-clap-state.md) の `.vvp` のような包み直しが要らない
- factory 音色は `.clap` に埋め込まれていてディスク上に無く、preset-discovery factory は NULL。
  host が列挙できる形にするには、どこかからファイルとして持ってくるしかない
- 取得元の commit を descriptor version から決めるので、plugin を更新したら catalog を作り直すだけで追従する

## 却下した案

| 案 | 却下理由 |
|---|---|
| CLAP preset-load（PLUGIN location + index） | 名前もカテゴリも返らず、index は plugin の更新でずれる。[0006](0006-no-generic-clap-preset-api.md) の方針にも反する |
| factory を user に手でコピーしてもらう | plugin を更新するたびに作業が要り、忘れると本体と食い違う |
| repo 全体の zip（codeload）を 1 回で取る | 音色以外も含めて約 8.8MB。展開に zip crate が要る |
| `.clap` のバイナリから埋め込み資源を抜き出す | 埋め込みの形式（cmrc）は plugin の公開契約ではなく、build のたびに変わりうる |

## 罠

- **state の反映は次の `process()` の先頭で、そのとき鳴っている音を全部止める。** ロード直後と同じブロックの
  note-on は鳴らない。ロードのたびに空ブロックを 1 回回してから返す（activate 前の経路も同じ）。
  呼び出し側の settle に頼ると、settle 0 の経路（scheduled 再生の頭）で 1 音目が無音になる
- **照合は両方向に要る。** `.sxsnp` を他 plugin へ送る経路（`ensure_six_sines_capable`）と、
  `.fxp` のような汎用 state file を Six Sines へ送る経路（`ensure_accepts_generic_state_file`）の両方で、
  送る前に拒む。生の XML が他 plugin の state load へ届いたときの挙動は plugin ごとに違い、
  [0014](0014-vvp-as-clap-state.md) の Vaporizer2 はプロセスごと落ちた
- **descriptor は 2 件**（`org.baconpaul.six-sines` と多出力版 `.seven-outs`）。組み込みプロファイルに
  `plugin_id` が無いと instance を作れない
- **取得の途中で失敗したら旧置き場と commit 記録を残す。** 一時置き場へ全件書いてから入れ替える。
  commit 記録は置き場の外（兄弟ファイル `six-sines-factory.commit`）に置く。置き場の中だと列挙に紛れる
- mono/poly は param id `523`（`OutputNode` の playMode）で読む。param の並びは id 順ではないのでファイル全体を走査する。
  **読めないときは `Unknown`**（黙って Poly にすると Mono の音色が和音行へ出る）

## 壊れたら気づく場所

| テスト | 落ちたら |
|---|---|
| `core-lib/src/six_sines_factory/tests.rs`（ネット無し） | 同じ commit で再取得する・途中失敗で旧版が消える・`truncated` の tree を受け入れる・version に hash が無いのに進む |
| `six_sines_factory::tests::real_factory_is_downloaded_once`（`#[ignore]`・実ネット） | GitHub 側のパスや API が変わった |
| `core-lib/src/render/tests/six_sines.rs`（`#[ignore]`・実 plugin） | state の形式が変わって無音になる・切り替えで前の音色が残る・activate 前ロードの 1 音目が鳴らない・照合が外れた・`plugin_id` で main descriptor が選ばれない |
