# clap-mml-play-server

### 用途
- clap-mml-render-tui からライブラリとして利用します。
- clap-mml-render-tui からサーバープロセスとして起動して利用します。

### install

```
cargo install --force --git https://github.com/cat2151/clap-mml-play-server
```

### 経緯：
- 元repo（clap-mml-render-tui）からcloneして暖簾分けしました。暖簾分け断面までの履歴を持っています。

### 備忘：
- 実際のserver / CLI / TUI 機能は、clap-mml-render-tui 側で実現しています
  - → 最近こちらにserverプロセスを切り出し中です
