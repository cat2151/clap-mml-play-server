# ADR 0006: CLAP 汎用 preset API を採らない

- 状態: 採用（2026-08-20）
- 関連: [0004](0004-syx-format-and-persistent-ids.md) / [0007](0007-patch-string-decides-the-plugin.md)

## 背景

CLAP には preset の一覧・選択 API が対で存在する（`clap_preset_discovery_factory` と
`clap.preset-load` extension）。仕様上は「cartridge = container file、program index = `load_key`」を
素直に表現できる。しかし**両方 optional**で、実体を直接 probe した結果:

| 対象 | preset-discovery factory | preset-load extension |
|---|---|---|
| Dexed 1.0.1 | 安定 ID / `draft-2` とも **NULL** | `clap.preset-load` / `/draft-2` / `.draft/2` すべて **NULL** |
| Surge XT 1.3.4 | `…-factory/draft-2` のみ OK | `clap.preset-load.draft/2` のみ OK |

## 採らない理由

1. **Dexed は両方無く、CLAP 経由では 1,056 programs を 1 件も列挙・選択できない。**
   `.syx` 自前パース + SysEx 以外に手段が無い
2. Surge も draft ID のみで、generic 化しても plugin ごとの互換分岐が残り見返りが小さい
3. preset-load は **plugin 主導・非同期**（完了は host extension の `loaded` / `on_error`）で、
   16 instance 並列・offline render の再現性・cache key・MML metadata への永続保存という
   本プロジェクトの要件（**host 側の値型と決定的な同期完了**）に合わない

## 落とし穴

**Dexed バイナリ内に `clap.preset-load` の文字列は存在する。** これは
`clap-juce-extensions` wrapper 側のコードで、Dexed が opt-in していないだけ。
**文字列検索で能力を判定してはならない。**

## 将来の移行口

Dexed が preset-discovery を実装したら、cartridge / program をそのまま location + `load_key` へ
対応付けられるので、**adapter 内の catalog 実装だけを差し替えれば** generic 経路へ移行できる。
