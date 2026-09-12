# ADR 0015: Sforzando の任意 SFZ は vendor state adapter でロードする

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
`false` だった。この API は将来 factory preset を catalog に加える場合だけ別機能として使える。

## 番人テスト

- `server-config/src/sforzando_programs/tests.rs` — user root、manifest、traversal、競合、583 件実機 catalog
- `core-lib/src/sforzando/tests.rs` — CEGP/zlib/XML/template/escape
- `core-lib/src/render/sfz_state/tests.rs` — plugin identity
- `core-lib/src/render/tests/sforzando.rs` — 初期ロード・runtime 切替・拒否後の復帰・offline の
  実機音声が非無音であること（ignored）
