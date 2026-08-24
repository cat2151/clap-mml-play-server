# ADR 0010: `SURGE_DATA_HOME` 最適化は Surge 限定 / プラグイン同定の優先順位

- 状態: 採用（2026-08-20）
- 関連: [0007](0007-patch-string-decides-the-plugin.md)

## `SURGE_DATA_HOME` 最適化

`apply_minimal_surge_data_home()` は Surge の起動を **8.8 秒 → 0.9 秒**にしている。

**`std::env::set_var` を使うので、スレッドを spawn する前に呼ぶ必要がある。**
混在後は判定が「**プロセス内に Surge が 1 つでもあれば適用する**」へ変わった
（`apply_surge_data_home_for(kinds)`）。

### 採らなかった代替案

**「`load_entry()` を先に呼んで descriptor を見てから `set_var`」**:
Surge の `clap_entry.init()` が data home を読む可能性があり、読んでいた場合に最適化が無効化される。
**実測せずに採用しないこと。**

## プラグイン同定の優先順位

**config の `plugin_id` があればそれだけで決める → 無いときだけ `plugin_path` のファイル名。**

### なぜファイル名判定を消さないか

production の `ServerConfig` は固定 Surge XT profile の `plugin_id` を解決済みで持つ。
ただし core の低レベル API と、`plugin_id` を省略した custom profile は `None` を渡せる。
その経路でも Surge の最適化を失わないため、ファイル名判定を fallback として残す。

TUI 側の `Config::is_surge_xt()` / `is_surge_xt_plugin()` も同じ規則。

## 罠

**`SURGE_XT_PLUGIN_ID` がこの repo 内 2 か所にある**
（`server-config/src/plugin_identity.rs` と `core-lib/src/surge_data.rs`）。
統合するなら `core-lib` → `server-config` の依存を足す形になるので**未着手**。
