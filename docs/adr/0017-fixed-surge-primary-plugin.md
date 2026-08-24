# ADR 0017: 共有 config の既定プラグインを Surge XT に固定する

- 状態: 採用（2026-08-24）
- 関連: [0007](0007-patch-string-decides-the-plugin.md) / [0008](0008-spare-instance-pool.md) / [0010](0010-surge-data-home-and-plugin-identity.md)

## 決定

TUI と server が共有する config.toml では、音色無指定の行を鳴らす既定プラグインを
Surge XT に固定する。`active_plugin` は廃止し、値が Surge XT でも設定エラーにする。

plugin 固有値は `[plugins.<名前>]` にだけ置く。組み込み Surge XT と
`[plugins."Surge XT"]` の差分を merge した結果が固定既定になる。他の profile は
混在 catalog の候補であり、追加しても既定を変更しない。

`active_plugin`, `plugin_path`, `plugin_id`, `patches_dirs` と用途別 role 7 項目が
トップレベルにあれば、未知キーとして無視せずまとめてエラーにする。この検査と固定 profile の
解決は `cmrt-server-config` が持ち、TUI と server の両方が同じ helper を使う。

## 回帰範囲を狭める内部仕様

`ServerConfig.plugin_path` / `plugin_id` / `patches_dirs` は削除せず、ロード後だけ意味を持つ
runtime view として残す。既存の plugin catalog、patch routing、CoreConfig、予備 instance pool は
この解決済み view を読み続ける。catalog の先頭 kind が既定という契約も変えない。

したがって今回変更するのは config の入力境界と既定 profile の解決だけであり、
複数 plugin のロード方式や patch 文字列による routing は変更しない。
