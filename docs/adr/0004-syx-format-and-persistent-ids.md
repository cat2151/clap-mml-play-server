# ADR 0004: `.syx` の形式と、program の永続 ID

- 状態: 採用（2026-08-20）
- 関連: [0003](0003-dexed-program-change-guard.md) /
  clap-mml-render-tui `docs/adr/0001-patch-string-decides-the-plugin.md`

## `.syx`（DX7 32-voice bulk dump）の形式

- **4,104 bytes 固定**。ヘッダ `F0 43 0n 09 20 00`、末尾 `F7`
- checksum は**データ部 7bit 総和の 2 の補数**。手元の 33 件すべてがこの式で一致
- program 名は packed voice（128 bytes）の**末尾 10 bytes、固定長 ASCII**。重複し得る

実装は `core-lib/src/dx7/`（`cartridge.rs` / `patch_path.rs` / `voice.rs`）。

## 決定: 永続 ID は名前にしない

program 名は重複しうるので、永続 ID は
**「cartridge の root 相対 path + 0-based program index」**にする。

```
SynprezFM/SynprezFM_01.syx/01 Say Again.
└ サブディレクトリ ┘└ cartridge ┘└ program ┘
                                 └ 0-based index を 2 桁 ┘
```

- **表示にも永続 ID にも同じ文字列を使う。** したがって sanitize の規則
  （制御文字と path 区切りを空白へ潰して trim、全部空なら `(no name)`）を後から変えてはいけない
- **program 番号は 0 始まりの 2 桁固定。** この文字列がそのまま永続 ID になるので
  UI で 1-32 表示にしてはいけない
- **パース側は名前を見ない**（sanitize の規則を後から変えても、保存済みデータが指す program が
  変わらないように）

## ライセンス

**`.syx` parser は Dexed（GPL）のコードを 1 行も持ち込まず、公開されている DX7 SysEx 仕様から
独自実装した。** Dexed 本体は GPLv3 だが、外部インストール済み binary を動的ロードするだけなら
ソースを取り込まない。

## 罠

**Dexed の CLAP state は `dexedState` XML を JUCE binary XML にしたもので、
`.syx` と CLAP state は別形式。** Surge 用の「パッチファイルを少し剥いで state load」を
`.syx` へ流用してはいけない。

## 壊れたら気づく場所

- `core-lib/src/patch_list_tests.rs::installed_cartridges_all_parse` — 実物 cartridge のパース
