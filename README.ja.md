# clap-mml-play-server

### 用途
- clap-mml-render-tui からライブラリとして利用します。
- clap-mml-render-tui からサーバープロセスとして起動して利用します。

### install

```
cargo install --force --git https://github.com/cat2151/clap-mml-play-server clap-mml-realtime-play-server
```

### self update

どちらのサーバーから実行しても、`clap-mml-render-server` と
`clap-mml-realtime-play-server` の両方をまとめて更新します。

```
clap-mml-render-server update
```

または

```
clap-mml-realtime-play-server update
```

ビルド時のcommit hashとremote mainのcommit hashを比較するには、
各コマンドの `check` サブコマンドを使用します。

```
clap-mml-render-server check
clap-mml-realtime-play-server check
```

### 経緯：
- 元repo（clap-mml-render-tui）からcloneして暖簾分けしました。暖簾分け断面までの履歴を持っています。

### 備忘：
- 実際のserver / CLI / TUI 機能は、clap-mml-render-tui 側で実現しています
  - → 最近こちらにserverプロセスを切り出し中です
