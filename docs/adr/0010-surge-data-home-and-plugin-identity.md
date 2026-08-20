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

既定で生成される config は `active_plugin` を書かないので `plugin_id` は `None` になる。
ここでファイル名判定を消すと、**既存ユーザーの Surge が最適化を失って起動が 0.9 秒 → 8.8 秒へ戻る。**

`plugin_id` を持たない config は「`active_plugin` が無かった時代のもの（＝ Surge 専用）」と
「`[plugins.*]` に `plugin_id` を書かなかったもの」のどちらかなので、
**path で見ないと後者を Surge と誤判定する。**

TUI 側の `Config::is_surge_xt()` / `is_surge_xt_plugin()` も同じ規則。

## 罠

**`SURGE_XT_PLUGIN_ID` がこの repo 内 2 か所にある**
（`server-config/src/plugin_identity.rs` と `core-lib/src/surge_data.rs`）。
統合するなら `core-lib` → `server-config` の依存を足す形になるので**未着手**。
