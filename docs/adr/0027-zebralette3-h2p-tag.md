# ADR 0027: Zebralette3: `.h2p` を `:z` のタグで受け、ロード前に `#AM=` で製品を照合する

- 状態: 採用
- 決定者: user（タグで所有者を分けること・marker は検証専用・置き場は registry だけ）。
  `PatchForm::H2p` への改名、製品表の置き場、`has_builtin_effects()` と voicing の値は AI の設計
- 関連: [0023](0023-tyrelln6-h2p.md) / [0026](0026-patch-tag-decides-the-owner.md)

## 決定

1. `.h2p` の loader は `PatchForm::H2p` 1 つ。読む製品は `server-config/src/u_he_products.rs` の
   `U_HE_PRODUCTS`（TyrellN6 = 空タグ、Zebralette3 = `z`）に並べ、patch 文字列は `x.h2p:` / `x.h2p:z`
2. 製品名 1 つを、registry の `Software\u-he\<製品>`・音色置き場の `Presets\<製品>`・`#AM=<製品>` に共通で使う
   （`plugin-presets/src/u_he.rs`）
3. `.h2p` を送る前に、ファイルの `#AM=` が載っている plugin の製品名と一致するかを host 側で照合し、違えば送らない
   （`core-lib/src/h2p.rs` の `check_h2p_model`）。instance 生成時・`set_patch()`・catalog 向け state 準備の全部で
4. Zebralette3 の `has_builtin_effects()` は `true`、voicing は `AssumePoly`
5. 表示名は Zebralette3 だけ `:z` を残す（`1 Poly Keys/x.h2p:z`）。client の音色一覧は plugin のタグを表示名へ足す

## 理由

- plugin は他製品の `.h2p` を拒まず `Ok` を返して state を書き換える（TyrellN6 ⇄ Zebralette3 の両向きで実測）。
  取り違えを止められるのは host の照合だけ
- 3 つの名前は factory preset 全件（Zebralette3 382 件・TyrellN6 669 件）で製品名と一致した
- Zebralette3 の factory preset は 382 件中 332 件が `#Reverb=1` か `#Delay=1`。TyrellN6 は 0 件
- `probe-voicing` は `Features: Mono` の preset でも Poly と判定した。mono を読む根拠が無い

## 却下した案

| 案 | 却下理由 |
|---|---|
| Zebralette3 用に `PatchForm` を足す | 拡張子 1 つに loader 2 つになり、`patch_form_of_path()` が拡張子から決められない |
| `PatchForm::TyrellN6` の名前のまま使う | 名前が読む製品を 1 つに見せ、TyrellN6 専用の分岐と区別できない |
| `/*@Meta` の `Features:` で mono/poly を決める | `probe-voicing` の実測と合わない |

## 番人テスト

- `the_model_marker_must_name_the_product`（`#AM=` の照合が両向きで効く）
- `every_u_he_product_tag_is_in_the_table`（plugin 側のタグと patch タグの表が食い違わない）
- `the_display_tag_splits_h2p_between_tyrelln6_and_zebralette3`（表示名の `:z` で routing が分かれる）
- `the_other_products_h2p_is_refused_in_both_directions`（`#[ignore]`・実 plugin 2 つ）
