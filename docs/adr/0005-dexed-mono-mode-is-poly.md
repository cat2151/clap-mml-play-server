# ADR 0005: Dexed の `MonoMode` は既定 POLY。生成時に設定しない

- 状態: 記録（2026-08-20 実測）
- 関連: clap-mml-render-tui `docs/adr/0008-voicing-per-patch.md`

## 実測

`MonoMode` は **parameter index 3**（表示値 `MONO` / `POLY`、値 1 / 0）。
**cartridge program の属性ではなく instance state の parameter。**

| 状態 | 値 |
|---|---|
| instance 生成直後 | 0.0 = `POLY` |
| cartridge program を選んだ後（3 program で確認） | 0.0 = `POLY` |
| `set_patch(None)`（= CLAP state load）の後 | 0.0 = `POLY` |

## 決定

**instance 生成時に `MonoMode` を設定する必要は無い。**

## なぜこれが重要か

TUI 側は Surge 以外のプラグインの patch を**すべて poly とみなす**
（`VoicingPolicy::AssumePoly`）。これは Dexed については実測に基づく事実であって、
推測ではない。

## 壊れたら気づく場所

- `core-lib/src/render/tests/cartridge.rs::dexed_mono_mode_stays_poly_for_every_program`（`#[ignore]`）
  — **落ちたら TUI 側の `AssumePoly` の前提が崩れている**
