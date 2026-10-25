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

### 音色の鍵盤追従の測定

1音色を鳴らし、noteを変えると音高がついてくるか（`tracks_keyboard`）を測れます。pluginは問いません。音声デバイスは開きません。drum/percussion欄の全音色の一括計測はclap-mml-render-tuiの`cmrt build-pitch-tracking`が行い、このサブコマンドを1音色ごとに起動します。

```powershell
cargo run --release -p clap-mml-realtime-play-server -- probe-pitch-tracking --patch "patches_factory/Percussion/Drum One.fxp" --json
```

判定条件、制限、共有データ（`cat-music-patterns/patch-pitch-tracking.json`）までの流れは[音色の鍵盤追従の測定](docs/patch-pitch-tracking.md)を参照してください。

### 経緯：
- 元repo（clap-mml-render-tui）からcloneして暖簾分けしました。暖簾分け断面までの履歴を持っています。

### 備忘：
- 実際のserver / CLI / TUI 機能は、clap-mml-render-tui 側で実現しています
  - → 最近こちらにserverプロセスを切り出し中です
