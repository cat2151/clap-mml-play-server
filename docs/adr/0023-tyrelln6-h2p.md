# ADR 0023: TyrellN6: `.h2p` は CLAP state として流し、音色置き場は registry から読む

- 状態: 採用
- 関連: [0006](0006-no-generic-clap-preset-api.md) / [0007](0007-patch-string-decides-the-plugin.md) /
  [0018](0018-patch-load-must-not-spin-the-plugin.md) / [0022](0022-six-sines-sxsnp-state-and-github-factory.md)

## 決定

1. TyrellN6 の音色ファイル `.h2p` は、**バイト列を包み直さずにそのまま `clap.state` へ渡す**
   （`core-lib/src/render/tyrelln6_patch.rs`）。1 ファイル = 1 音色
2. `process()` を回したあとの instance へロードしたときは、`max(4, ceil(4096 / buf_size))` ブロック
   空回ししてから返す。`activate()` 前のロードでは回さない
3. 音色置き場は registry `HKCU\Software\u-he\TyrellN6` の `DataPath` の下の `Presets\TyrellN6` だけ
   （`server-config/src/vendor_patch_dirs.rs`）。config の `patches_dirs` は無視し、`UserPresets` は列挙しない
4. `.h2p` は拡張子で `PatchForm::TyrellN6` と決め、送り先は TyrellN6 に限る。他の u-he plugin の `.h2p` は扱わない
5. mono/poly は読まず `AssumePoly`。`has_builtin_effects()` は `false`（auto reverb 試聴の対象）

## 理由

- TyrellN6 の `save_plugin_state()` は `.h2p` と同じテキスト形式（`#AM=TyrellN6` ... `#cm=` ... 末尾に
  checksum 行と NUL）で、`.h2p` をそのまま渡すと Ok を返し、音色の param が state に載る
- 置き場を plugin 本体の登録から読むのは Vaporizer2 と同じ理由（再インストールのたびに toml を書き直さない）。
  置き場を 2 つにすると共通の親（display 文字列の基点）が無くなる
- `#cm=VCC` の `Voicing=` は 1〜8 の値で mono/poly の 2 値ではなく、読む根拠にならない
- 載っている effect は chorus だけで reverb が無い

## 却下した案

| 案 | 却下理由 |
|---|---|
| CLAP preset-load（`clap.preset-load/2` と preset-discovery factory は実装されている） | [0006](0006-no-generic-clap-preset-api.md) の方針。列挙も選択も host 側で持てる |
| `/*@Meta` の `Categories` から Role を決める | 表示パス（`01 Basses/...`）の汎用分類で足りる |
| config の `patches_dirs` を置き場にする | 本体の登録と食い違ったとき気づけない |

## 罠

- **state の反映は、`process()` を回したあとの instance では数ブロックかけて寄っていく。** 無音にはならないが、
  空回しなしの 1 音目は前の音色寄りの音になる（buffer 512 で包絡の差 0.23 → 8 ブロック後 0.003）。
  frame 数だけでもブロック数だけでも揃わず、buffer 2048 では 4 ブロック要る
- **同じ音色でも、打鍵の履歴で音が変わる。** `reset()` と settle では新しい instance の音に揃わない。
  切り替えで前の音色が残っていないかは、音ではなく state のバイト一致で見る
- **照合は両方向に要る。** `.h2p` を他 plugin へ送らない（`ensure_tyrelln6_capable`）、`.fxp` 等の汎用
  state file を TyrellN6 へ送らない（`render/patch_state.rs` の `ensure_accepts_generic_state_file`）
- `.h2p` は途中に NUL を含む。テキストとして読み直さず、バイト列のまま扱う
- 生成のたびに plugin が stdout へ `log path:` の行を出す。ファイルは作られない

## 壊れたら気づく場所

| テスト | 落ちたら |
|---|---|
| `render::tyrelln6_patch::tests` / `render::patch_state::tests`（plugin 不要） | 照合が外れた |
| `server-config/src/vendor_patch_dirs/tests.rs` | `DataPath` から置き場を組み立てる規則が変わった |
| `core-lib/src/render/tests/tyrelln6.rs`（`#[ignore]`・実 plugin） | descriptor・state の形式が変わった・`.h2p` をそのまま読まなくなった |
| `core-lib/src/render/tests/tyrelln6_patch_switch.rs`（`#[ignore]`・実 plugin） | 全音色のロード失敗・切り替えで前の音色の state が残る・空回しが足りず settle 0 の 1 音目が前の音色寄りになる |
