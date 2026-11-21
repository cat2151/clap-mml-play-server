# ADR 0006: CLAP 汎用 preset API は原則採らない

- 状態: 採用（Apricot の preset API 利用は例外）
- 関連: [0001](0001-measured-plugin-capabilities.md) / [0004](0004-syx-format-and-persistent-ids.md) /
  [0007](0007-patch-string-decides-the-plugin.md) / [0014](0014-vvp-as-clap-state.md)

## 背景

CLAP には preset の一覧・選択 API が対で存在する（`clap_preset_discovery_factory` と
`clap.preset-load` extension）。仕様上は「cartridge = container file、program index = `load_key`」を
素直に表現できる。しかし**両方 optional**で、実 probe（[0001](0001-measured-plugin-capabilities.md) の表）では
Dexed と Vaporizer2 が両方 **NULL**、Surge XT と Sforzando は安定 ID で opt-in している
（Surge は「使えるが使っていない」）。

## 採らない理由

1. **Dexed と Vaporizer2 は両方無く、CLAP 経由では 1 件も列挙・選択できない。**
   Dexed は `.syx` 自前パース + SysEx、Vaporizer2 は `.vvp` 自前列挙 + state ロード以外に手段が無い
2. **載っていないプラグインがある以上、generic 化しても plugin ごとの互換分岐が残る。**
   Surge だけのために経路を二重に持つ見返りが無い
3. preset-load は **plugin 主導・非同期**（完了は host extension の `loaded` / `on_error`）で、
   16 instance 並列・offline render の再現性・cache key・MML metadata への永続保存という
   本プロジェクトの要件（**host 側の値型と決定的な同期完了**）に合わない
4. 列挙できても**用途別カテゴリ・mono/poly の材料は結局ファイル側にある**
   （Vaporizer2 はファイル名の 2 文字コードと `m_uPolyMode`。[0014](0014-vvp-as-clap-state.md)）

## 落とし穴

**Dexed / Vaporizer2 のバイナリ内に `clap.preset-load` の文字列は存在する。** これはどちらも
`clap-juce-extensions` wrapper 側のコードで（Vaporizer2 の `VASTAudioProcessor` は
`clap_juce_audio_processor_capabilities` を継承していない）、プラグインが opt-in していないだけ。
**文字列検索で能力を判定してはならない。**
判定は [0001](0001-measured-plugin-capabilities.md) の `probe-capabilities` で実 probe する。

## 将来の移行口

Dexed / Vaporizer2 が preset-discovery を実装したら、cartridge / program や `.vvp` を
そのまま location + `load_key` へ対応付けられるので、**adapter 内の catalog 実装だけを
差し替えれば** generic 経路へ移行できる。

## Apricot は plugin 専用 adapter から preset API を使う

当初の対象では、API が無い plugin に合わせたファイル列挙と state 変換で要件を満たせた。
Apricot 1.1.34 の factory 90 音色は本体に内蔵され、user の Patches folder にもファイルが無い。
一方、実 API を呼ぶと `clap.preset-discovery-factory/draft-2` が名前付きの `.synthpatch` load key を返し、
`clap.preset-load.draft/2` で選択できた。全 factory の切替と、選択直後の state を別 instance に
読み込んだ結果の一致を確認できたので、Apricot に限ってこの対を利用する。

factory の参照は bundle を container とする仮想パスにし、user preset は本体の Documents 内の
保存先から列挙する。factory は PLUGIN location、user file は FILE location へ送り、
外部 host に渡すときは native instance で読み込んで owned state にする。
実体のない factory file を通常のファイル読み込みへ渡してはいけない。
catalog 構築時には provider をロードするが、server 起動時の基点解決では列挙しない。

これは汎用 API への移行ではない。optional API の有無と同期完了の根拠は今後も plugin ごとに確認する。
埋め込み資源を解析して抜き出す方式は公開契約が無く、音色を別途配布・保存する必要もないため採らない。
既存 plugin の参照と保存形式は維持し、新規 Apricot の追加に旧データの移行は要らない。

## Sforzando も generic preset API の例外にしない

Sforzando の factory preset は PLUGIN location + factory key で preset-load できるが、任意 `.sfz` の
FILE location は stable / draft とも `false`。任意 SFZ は registry / bank manifest で検証した
ARIA program 座標から vendor state を構築し、同期的な `clap.state.load` を使う。詳細は
[0015](0015-sforzando-sfz-preset-load.md)。
