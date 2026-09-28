# ADR 0015: Sforzando の任意 SFZ と `.ariax` は vendor state adapter でロードする

- 状態: 採用
- 関連: [0001](0001-measured-plugin-capabilities.md) / [0006](0006-no-generic-clap-preset-api.md) /
  [0007](0007-patch-string-decides-the-plugin.md)

## 決定

任意の `.sfz` は `clap.preset-load/2` の FILE location ではロードしない。canonical absolute path を
ARIA の program source で検証済みの `(bank_id, bank_version, program_name)` へ解決し、instance 生成直後に
保存した state を template に Sforzando 固有 state を構築して `clap.state.load` へ渡す。

- Windows の user bank は registry `user_files_dir` と canonical containment を照合する。この source が
  存在するときだけ、実測した user-bank 座標 5000 / 1000 と root-relative program name を使う
- installed bank は `*.bank.xml` の `AriaBank/@id`, `@version`, `AriaProgram/@name`,
  `AriaElement/@path` を使う。ファイル名から program name を推測しない
- manifest 未登録、root 外参照、異なる program が同一 canonical path を指す競合は catalog と loader の
  両方で拒否する。`state.load == true` を mapping 成功の代用にしない
- renderer は plugin ID を確認し、program 解決と state 構築を processor 停止前に済ませる。state load 後は
  成否にかかわらず processor restart を試し、load と restart の両方が成功した場合だけ `current_patch` を更新する
- MML/IPC/history/session には従来どおり相対 `.sfz` path だけを保存し、ARIA 座標を露出しない

## 音色置き場は ARIA の registry だけで決める

走査の root は user bank（HKCU `user_files_dir`）と installed bank（HKLM `...\Aria\Products\<id>` の
`vendor` / `product` から引く `...\<vendor>\<product>\Banks\*\bank_path` の親）だけ。
`[plugins.Sforzando] patches_dirs` は読まない。state が program を bank 座標でしか指せないので、
ARIA に登録されていない場所は一覧に出せても鳴らせない。Banks を持たない Product（sforzando 本体）は黙って飛ばし、
registry にあるのに実在しない `bank_path` は notice にする。

- 却下: toml の dir を root に足す案。registry と同じ場所を重ねるだけか、鳴らせない場所を足すだけで、
  dir の移動後に古い値が残っても気づけない
- registry が無い OS では root が 0 件になる

## `.ariax`（ARIA の preset）も同じ state 経路で載せる

`.ariax` は平文の AriaSave XML で、違いは `Slot` の `Param` だけ。init template の `Slot` 要素を `.ariax` の
`Slot` で丸ごと置き換え、`Settings` / `EffectSlot` / `GUI` は template のものを使う（offline の `streaming=0` も
template 側へ入れる）。

- `Slot@name/@bankId/@version` は、近傍の `*.bank.xml` の `AriaProgram` と一致したときだけ採用する。
  一致しなければ load をエラーにする（`.sfz` と同じく `state.load == true` を成功の代用にしない）
- catalog には、音色置き場の下の `.ariax` のうち同じ照合を通るものだけを載せる。通らないもの（別環境の
  program を指す preset、属性欠け、壊れた XML）は user の側で直せないので notice を出さずに除外する
- 切替では target の前に init state を load する。ARIA は同じ program が載ったままだと `Param` を持ち越し、
  `.sfz` の state は `Param` を持たないので、`.ariax` → 同じ program の `.sfz` が preset の音のままになる
- 却下: preset-load（PLUGIN location）で factory preset を読む案。plugin 主導の非同期完了で、同期の state
  経路の要件（[0006](0006-no-generic-clap-preset-api.md)）に合わない

## 相対 path の基点は音色置き場ごと

音色置き場（user bank、installed bank ごとのディレクトリ）は registry から互いに独立に決まり、別ドライブにも
置かれる。共通の親は偶然の産物で、無いこともある。そこで Sforzando だけ基点を置き場ごとに持つ
（`PatchBase::PerRoot`）。相対 path は各置き場の**親**からの相対で、先頭要素が置き場のフォルダ名になる
（例 `sfz/...`、`Free Sounds/Programs/...`、`TableWarp2/...`）。解決は先頭要素とフォルダ名が一致する置き場を選ぶ。

- 一致しない先頭要素は解決せず入力のまま返し、load で失敗させる。共通の親を基点にしていた頃の形
  （例 `Plogue/Free Sounds/...`）への fallback は持たない
- 却下: 全置き場の共通の親を基点にする案。置き場が別ドライブにあると基点が無くなり、同じドライブでも
  置き場の追加・移動で全 path の形が変わる
- 同じフォルダ名の置き場が 2 つあると、先に並ぶ方にしか解決できない

## state format と template

state container は `CEGP` magic、展開後 XML byte length の little-endian `u32`、zlib 圧縮した
`AriaSave` XML の順。length は圧縮後ではない。codec は magic、header、length、zlib、UTF-8、root element を
個別に検証する。XML writer に属性 escape を任せる。

Sforzando 2.1.2.4 / ARIA Engine 1.982 の空 instance が保存する template は
`AriaSave/Settings`, `EffectSlot`, `GUI` を含むが `Slot` を含まなかった。Settings 等を固定 template で
上書きせず、そのまま保持する。`Slot` があれば name/bankId/version だけを更新し、無ければ同版で実測した
最小 Slot（id=0, channel=-1, poly=32, tuning/transposition=0, `Main value=1`）だけを EffectSlot 前へ挿入する。
template が CEGP/AriaSave でなければ固定 state へ fallback せず具体的な error にする。

## 採らなかった案: preset-discovery / preset-load

provider は filesystem `.sfz` location ではなく PLUGIN location `factory` を 1 件公開し、TableWarp2 の
factory preset 36 件を返した。factory key（例 `3103/com.Plogue.Aria/Keys/Space Flute`）は
`preset-load(PLUGIN, key)` でロードできる。一方、任意 SFZ は stable / draft/2 の FILE location の双方で
`false` だった。factory preset は installed bank の下の `.ariax` file として catalog に載せる。

## 番人テスト

- `server-config/src/sforzando_programs/tests.rs` — user root、installed bank の registry 読み取り結果、manifest、traversal、競合、実機 catalog 件数
- `server-config/src/patch_base/tests.rs` — 置き場ごとの基点の解決・相対化・fallback しないこと。実機の全 program の往復は `patch_catalog/tests.rs`（ignored）
- `core-lib/src/sforzando/tests.rs` — CEGP/zlib/XML/template/escape
- `core-lib/src/render/sfz_state/tests.rs` — plugin identity
- `server-config/src/sforzando_programs/tests/ariax.rs` — `.ariax` の Slot 座標と manifest の照合、catalog に載る `.ariax` の選別。
  実機の TableWarp2（`.sfz` 1 + `.ariax` 36）は `tests/installed.rs`（ignored）
- `core-lib/src/render/tests/sforzando.rs` — 初期ロード・runtime 切替・拒否後の復帰・offline の
  実機音声が非無音であること、`.ariax` の Param が音と保存 state に残ること、`.sfz` ↔ `.ariax` の切替で
  それぞれの音に戻ること（ignored）
