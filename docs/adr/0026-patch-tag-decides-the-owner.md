# ADR 0026: patch 文字列は `<パス><拡張子>:<タグ>`。拡張子が loader を、タグが所有者を決める

- 状態: 採用
- 決定者: user（文法・タグの文字種と上限・短いタグから割り当てる規則・正規形・未知はエラー・marker は検証専用・JSON キーの改名）。
  表と境界関数の置き場、CacheWav にも `:` を付けること、SHM の VERSION 上げは AI の設計
- 置き換え: [0007](0007-patch-string-decides-the-plugin.md)
- 関連: [0023](0023-tyrelln6-h2p.md)

## 決定

1. patch 文字列（wire）は `<パス><拡張子>:<タグ>`。区切りの `:` は必須で、タグは空でよい。
   MML 先頭 JSON・`patch_history.txt`・config の `patch_path`・live IPC・cache WAV
   （`slot=1;D:\...\x.wav:` / `slot=1;:`）のすべてで同じ
2. タグは `[a-zA-Z0-9]*`、大文字小文字を区別、16 文字まで。拡張子ごとのスコープで、
   既定所有者は空タグだけで書く（`.h2p:` = TyrellN6）。既定所有者を名前で書く（`.h2p:t`）のはエラー。
   正規形は 1 つ。非空タグの割り当ては 1 文字から始め、同じ拡張子のスコープで使える 1 文字の候補を
   使い切ったときだけ 2 文字へ進む。以降も短い長さの候補を使い切ってから次の長さへ進む。
   製品名の頭文字が衝突しても、未使用の 1 文字があればその中から選ぶ。
   製品名や略称を表すために最初から複数文字を割り当てない。16 文字は文法上の上限であり、割り当てる長さの自由裁量ではない
3. 解析は `core-lib/src/patch_tag.rs` の `parse_patch_wire()` だけで行う。末尾の `:` で切り、
   その後ろにパス区切りがあればドライブ文字の `:` とみなして旧形式エラー。未登録のタグ・未知の拡張子もエラー。
   拡張子の catch-all（未知は Surge の state file）は持たない
4. 拡張子は loader（`PatchForm`）を、タグは所有者（`PluginKind` の選択と基点）を決める。
   `kind_for_patch()` と `PatchBases` は (形, タグ) で引く
5. server 内部では「パス部分だけ絶対パスへ直した wire」を運び、`set_patch()` と instance 生成時の
   ロードが剥離する。`Path`・`PatchBase`・`is_*_patch_path` には剥離後の文字列だけを渡す
6. MML 先頭 JSON のキーは `"patch"` / `"patch filter"`。旧キー `"Surge XT patch"` /
   `"Surge XT patch filter"` があればエラーにし、既定音色で鳴らさない。runtime に旧形式の互換 reader は置かない
7. `#AM=` などの model marker はロード時の内容検証にだけ使い、routing には使わない
8. SHM の `VERSION` を 14 に上げる（layout は同じ）

## 理由

- Zebralette3 の preset は TyrellN6 と同じ `.h2p` で、拡張子だけでは送り先が決まらない（0007 の「未解決の論点」）。
  実測では Zebralette3・TyrellN6 とも他製品の `.h2p` を拒まず state を書き換えるので、取り違えを plugin に任せられない
- タグを拡張子スコープにすると、既存の音色はすべて空タグで書けて、表示名（空タグの `:` を落とす）と
  favorites のキーが変わらない
- 当初は文字種と 16 文字の上限だけを明文化していたため、OB-Xf の追加で、1 文字の候補が残っていても
  製品の略称 `obxf` を割り当てられると解釈された。user の意図は 1 文字から始め、収まらなくなったときだけ
  長くすることだったため、文法上の上限と割り当て規則を区別して明記する
- 未タグ・未知を黙って既定へ流すと「操作は成功したが別の plugin の音」という静かな間違いになる。
  cache WAV だけ例外にすると「未タグ = エラー」が入口ごとにばらつく
- Windows の `Path` は `:` を ADS 区切りとして扱い `extension()` が崩れるので、剥離は 1 か所に集める
- `kind_for_patch()` の時点では相対参照の実ファイルを解決できない経路がある（render server は基点を選ぶ前に
  plugin を決める）ので、marker を読んで routing することはできない
- SHM の layout が同じでも、旧 server は `x.h2p:z` を拡張子だけで routing して別 plugin で鳴らす。繋がらないほうが安全

## 却下した案

| 案 | 却下理由 |
|---|---|
| marker（`#AM=`）で routing する | 相対パスのまま plugin を決める経路でファイルを読めない。読めても列挙・選択の度に I/O が要る |
| 製品名を拡張子に足す（`.z3.h2p`） | 実ファイル名と食い違い、display ⇄ 絶対パスの往復が壊れる |
| 製品名や略称を非空タグにして最初から複数文字を使う（`:obxf`） | 1 文字で収まる間は 1 文字を使うという user の割り当て方針に反する |
| 旧形式（`:` 無し）を空タグとして読む互換 reader | 残り続ける。旧データは一度だけの移行で書き換える |
| タグを wire の先頭に置く（`z:x.h2p`） | ドライブ文字・cache WAV の `slot=1;` と前置が衝突し、分解規則が増える |

## 番人テスト

- `every_form_has_exactly_one_default_owner`（全 `PatchForm` が既定所有者にちょうど 1 回）
- `longer_tags_require_all_shorter_tags_in_the_same_form_to_be_used`（同じ拡張子の短い候補を使い切る前に長いタグを登録させない）
- `untagged_patches_are_legacy_errors`
- `the_old_patch_key_is_an_error_instead_of_the_default_patch`
- `an_untagged_patch_is_a_legacy_error_instead_of_a_routing_guess`
- `cache_wav_wires_carry_the_empty_tag_and_untagged_spellings_are_legacy`
