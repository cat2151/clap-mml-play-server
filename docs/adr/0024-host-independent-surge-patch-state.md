# ADR 0024: Surge FXP の CLAP state 変換をホスト非依存 API として共用する

- 状態: 採用
- 関連: [0006](0006-no-generic-clap-preset-api.md)、[0016](0016-audio-plugin-catalog-api.md)

## 背景

cat-plugin-player の Random patch と cmrt で同じ Surge XT patch を使う。
既存の変換は clack-host instance の load 処理に埋め込まれており、uapmd から呼べない。
従来 decoder は短い header で panic し、chunk 長の切り詰めや任意位置の magic 探索、raw fallback も行っていた。
公開入口でこれらを許すと、不正ファイルが plugin の state.load に届く。

## 決定

`cmrt_core::prepare_clap_patch_state(plugin_id, absolute_patch_path)` と `PatchStateError` を公開する。
disk read と owned bytes の変換だけを行い、host・plugin・device を作らない。
正確な Surge XT CLAP ID と絶対 `.fxp` path を読み取り前に確認し、拡張子の ASCII 大小文字を許容する。
相対 path の補完や canonicalize を公開契約にしない。Io は path と元の error を保持する。

対応形式は 60 byte の `CcnK` / `FPCh` / `cjs3` header と offset 60 の `sub3`。
big-endian chunk 長がファイル境界と一致すること、32 byte sub3 header の little-endian XML 長と
6 個の wavetable 長が chunk 内に収まることを検査する。Surge の byteSize=0 は許容する。
切り詰め・探索・不正時の raw fallback はせず Err にする。XML の意味や適用成功は plugin の責務。

初回生成と RealtimeRenderer の切替は、Surge の `.fxp` に限って同じ関数を使う。
既存 cmrt の相対 path は既存の lexical_absolute で CWD 基準の絶対 path にする。
非Surgeと既存 raw state/XML の経路は維持する。新 crate・feature・error 依存や7音源対応を加えない。
TUI はこの同じ関数と error 型を再 export し、decoder を複製しない。

## 番人テスト

`core-lib/src/patch_state_prepare/tests.rs` に device 不要の合成 fixture を置く。

- `prepared_state_is_the_exact_surge_chunk`: 正常 chunk の完全一致と拡張子。
- `input_contract_is_checked_before_disk_read`: ID・絶対 path・形式の事前拒否。
- `truncated_headers_and_chunks_return_errors_without_panicking`: 特に60〜63 byteでのpanic防止。
- `invalid_magic_ids_and_declared_lengths_are_rejected`: magic・ID・chunk/XML/wavetable 境界の拒否。
- `disappeared_file_retains_io_source_and_path`: 消失時の Io 情報。

ignored test `real_catalog_surge_bytes_match_the_previous_normal_decoder` は
`CMRT_PATCH_CATALOG` を指定し、既存 catalog の Surge patch を読み取りだけで旧正常 decode と比較する。
私有 patch を repo へコピーしない。これは native 適用や有音の検証ではない。

既存 catalog では3170件がこの境界を満たす。
`patches_factory/Basses/Bass 3.fxp` はファイル長31984、宣言chunk長31923で
offset60 + chunk長より1byte余剰がある。sub3内部の宣言長は31923と一致する。
旧decoderは余剰byteを切り捨てるが、公開契約ではこの1件をInvalidDataとして拒否する。
実catalog比較では契約外入力をpathと理由付きで報告し、正常入力との等価を分けて記録する。
全catalog対応のために検査を緩める場合は、対応形式の拡張として別途判断する。
