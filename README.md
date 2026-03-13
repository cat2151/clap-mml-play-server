# clap-mml-render-tui

### 用途

- MMLで音を鳴らして遊ぶ用
- カジュアルにインストールする用。RustがあるだけでOK

### 技術スタック
- プラグインホスト用ライブラリ
  - https://github.com/prokopyl/clack

### 準備

Surge XTをinstallしてください

```
winget install "Surge XT"
```

### install

```
cargo install --force --git https://github.com/cat2151/clap-mml-render-tui
```

### 実行

```
cmrt
```

TUI画面でMML入力して遊べます

# 破壊的変更
- 毎日頻繁に破壊的変更します

# スコープ外
- effectはおそらく編集必須なので割り切って、ひとまずスコープ外、かなり後ろに後回し、とする
