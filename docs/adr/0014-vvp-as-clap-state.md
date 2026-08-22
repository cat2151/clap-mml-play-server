# ADR 0014: `.vvp` は CLAP state として流す（列挙も選択も host 側）

- 状態: 採用（2026-08-22）
- 関連: [0001](0001-measured-plugin-capabilities.md) / [0003](0003-dexed-program-change-guard.md) /
  [0006](0006-no-generic-clap-preset-api.md) / [0007](0007-patch-string-decides-the-plugin.md)

## 決定

Vaporizer2 の音色ファイル `.vvp` は、**中身の XML に 9 バイト被せて `clap.state` へ渡す。**

```rust
// core-lib/src/vvp.rs
let mut blob = Vec::with_capacity(xml.len() + 9);
blob.extend_from_slice(&0x2132_4356u32.to_le_bytes()); // JUCE の magic
blob.extend_from_slice(&(xml.len() as u32).to_le_bytes()); // 末尾 NUL を含まない長さ
blob.extend_from_slice(&xml);
blob.push(0);
```

単位は Surge XT の `.fxp` と同じ「1 音色 = 1 ファイル = 1 CLAP state」。

## 理由: `.vvp` の中身は CLAP state の中身そのもの

`VASTAudioProcessor.cpp`（<https://github.com/VASTDynamics/Vaporizer2>）を読んだ結果:

| 呼ばれ方 | 実装 |
|---|---|
| `getStateInformation`（CLAP state 保存） | `copyXmlToBinary(createPatchXML(true), destData)` (:934) |
| `savePatchXML`（`.vvp` を書く） | 同じ `createPatchXML(true)` (:576) |
| `setStateInformation`（CLAP state ロード） | `getXmlFromBinary()` → `PatchVersion` で分岐 (:940-960) |

つまり **CLAP state = `.vvp` の XML を JUCE の binary-XML で包んだもの**。
包み方は `juce_AudioProcessor.cpp:946-961`（magic 4 + 長さ 4 + UTF-8 XML + NUL 1）。

`isFromState=true` の経路は**同期ロード**（:1163 `passTreeToAudioThread(..., isSeparateThread=false, ...)`）。
一方 `.vvp` をファイル名で読ませる `loadPresetFile` は detached thread で非同期なので、
**state 経由のほうが競合が無い**。

## 採らなかった案: Program Change / preset-load

- **Program Change**: プラグインが起動時に非同期スキャンした preset 配列の index に依存する
  （`setCurrentProgram` → `loadPreset(index)`）。順序が環境依存で、`setChunk` 直後 400ms の
  ガードもある（:542）。**Dexed で同じ形の罠を踏んでいる**（[0003](0003-dexed-program-change-guard.md)）
- **CLAP の preset-discovery / preset-load**: Vaporizer2 3.5.0 は**どちらも NULL**
  （実 probe。[0006](0006-no-generic-clap-preset-api.md)）。列挙も選択もできない

## 罠 1: **V2.00000 の 50 件は版を読み替えないと誤解釈される**

`setStateInformation` は **`VASTVaporizerParamsV2.00000` のときだけ
`externalRepresentation=false`** でパースする（:954-955。コード中のコメント自体が疑問形）。
しかしファイルとして保存された `.vvp` は常に external 表現なので、そのまま流すと
**パラメータが誤解釈される。**

対策: blob を作る前に XML テキスト中の
`VASTVaporizerParamsV2.00000` → `VASTVaporizerParamsV2.10000` へ置換する。

- skew 補正は `preset.version` が **2.00000 と 2.10000 の両方**で掛かる（:1339-1340）ので
  置換しても補正は失われない
- **2.20000 には触らない**（skew 挙動が違う）
- 置換は**バイト列の同じ長さの差し替え**（`const _: () = assert!(len == len);` で固定）
- 内訳は V2.20000 = 375 件 / V2.10000 = 35 件 / V2.00000 = **50 件**

**この読み替えは「名前が state に入ったか」では検出できない**（名前も長さも変わらない）。
番人テストは**鳴らし比べ**にしてある（`retagging_a_v2_00000_patch_changes_what_it_sounds_like`）。

## 罠 2: **生の XML をそのまま state へ渡すとプロセスごと落ちる**

`load_patch()` は FXP ヘッダが無いバイト列を**素通しで** state へ渡すので、
`.vvp` の生 XML が Vaporizer2 の `setStateInformation` へ届きうる。
実測すると **STATUS_ACCESS_VIOLATION でテストハーネスごと落ちた**
（「静かに間違った音が鳴る」ではない）。

→ `.vvp` の分岐（`patch_switch.rs` / `render.rs` の activate 前 / `ensure_vvp_capable()`）は
**どれも省略できない**。落ちるテストは同居させられないので、
`core-lib/src/render/tests/vaporizer2.rs` にコメントだけ残してある。

## ヘッダは先頭 4096 バイトだけ読む

必要な情報（`PatchVersion` / `PatchName` / `PatchCategory` / `m_uPolyMode`）はすべて先頭にある。
**460 ファイル全読み（681MB・最大 1 ファイル 17MB）は絶対にしない。**

- 実測での `m_uPolyMode` の終端は最大 835 バイト目（`AR Comb ARP.vvp`）だが、
  `PatchComments` が長いユーザープリセットでは後ろへずれるので 4096 にしてある
- **届かなかったときは黙って既定値にせずエラー。** Mono を poly と誤ると和音行へ出てしまう
- 実測 460 件すべてで読めた（未判定 0 件・warm 40ms / cold 近似 51ms）
- `parse_vvp_header(prefix)` を `read_vvp_header(path)` と別に公開してある。
  ファイルを置かずに単体テストを書けるようにするため

`m_uPolyMode` の内訳: Mono 144 / Poly4 16 / Poly16 298 / Poly32 2。
**判定は「`Mono` か」であって綴りの一覧ではない**（新しい Poly 値が増えても poly 側）。
使い道は TUI 側 `docs/adr/0008-voicing-per-patch.md` の `VvpHeader` 方針。

## 音色置き場の既定値は持たない

Vaporizer2 のプリセット置き場は `%APPDATA%\Vaporizer2\VASTvaporizerSettings.xml` の
`PresetRootFolder` か HKLM のグローバル設定で決まる**ユーザー固有の値**なので、
`default_vaporizer2_plugin_path()` はあるが **`patches_dirs` の既定値は作らない**。

結果として `[plugins.Vaporizer2]` に `patches_dirs` を書くまで**カタログに載らない**
（音色置き場ゼロのプラグインは `catalog_plugins_with()` が飛ばす）。
**「音色 0 件」で倒れるのが正しい倒れ方**で、Surge の dir を流用すると
`.fxp` が Vaporizer2 の音色として一覧に出る。

**プラグイン側の設定ファイルもレジストリも読みにも書きにも行かない**（ユーザーの DAW 環境を壊す）。

## 壊れたら気づく場所

すべて `#[ignore]` + 実プラグイン（`core-lib/src/render/tests/vaporizer2.rs`）:

| テスト | 落ちたら |
|---|---|
| `vaporizer2_loads_every_patch_version_as_state_and_makes_sound` | 版 3 種 + 17MB のどれかが載らない・無音になる・名前が state に入らない |
| `retagging_a_v2_00000_patch_changes_what_it_sounds_like` | **罠 1 の読み替えが効いていない**（名前では検出できないので鳴らし比べ） |
| `set_patch_routes_a_vvp_path_to_the_vaporizer2_loader` | 公開 API `set_patch()` が `.vvp` を state 経路へ載せていない |
| `a_vvp_patch_in_the_config_is_loaded_before_activate` | config の `patch_path` に `.vvp` を書いた起動経路が壊れた |
| `a_vvp_patch_is_refused_before_it_reaches_surge` / `set_patch_refuses_a_vvp_path_on_a_surge_instance` / `a_surge_instance_refuses_to_start_with_a_vvp_patch_in_the_config` | **罠 2 の照合が外れた**（Surge がプロセスごと落ちる） |
