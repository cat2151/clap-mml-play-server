# ADR 0006: CLAP 汎用 preset API を採らない

- 状態: 採用（2026-08-20 / 2026-08-22 に Vaporizer2 の実測で再確認）
- 関連: [0001](0001-measured-plugin-capabilities.md) / [0004](0004-syx-format-and-persistent-ids.md) /
  [0007](0007-patch-string-decides-the-plugin.md) / [0014](0014-vvp-as-clap-state.md)

## 背景

CLAP には preset の一覧・選択 API が対で存在する（`clap_preset_discovery_factory` と
`clap.preset-load` extension）。仕様上は「cartridge = container file、program index = `load_key`」を
素直に表現できる。しかし**両方 optional**で、実体を直接 probe した結果:

| 対象 | preset-discovery factory | preset-load extension |
|---|---|---|
| Dexed 1.0.1 | 安定 ID / `draft-2` とも **NULL** | `clap.preset-load` / `/draft-2` / `.draft/2` すべて **NULL** |
| Surge XT 1.3.4 | **`…-factory/2` が非 NULL**（安定 ID） | **`clap.preset-load/2` と `.draft/2` の両方が非 NULL** |
| **Vaporizer2 3.5.0** | 安定 ID / draft とも **NULL** | 安定 ID / draft とも **NULL** |

2026-08-22 の再 probe で Surge の行を直した。初版は「draft ID のみ」と書いていたが、
**Surge は安定 ID でも opt-in している**。正確には「使えるが使っていない」。

## 採らない理由

1. **Dexed と Vaporizer2 は両方無く、CLAP 経由では 1 件も列挙・選択できない。**
   Dexed は `.syx` 自前パース + SysEx、Vaporizer2 は `.vvp` 自前列挙 + state ロード以外に手段が無い
2. **3 プラグイン中 2 つが載っていない以上、generic 化しても plugin ごとの互換分岐が残る。**
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
