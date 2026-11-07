# clap-mml-play-server

### Usage
- Used as a library by clap-mml-render-tui / cat-plugin-player / cat-smf-editor.
- Started and used as a server process by clap-mml-render-tui.

### install

```
cargo install --force --git https://github.com/cat2151/clap-mml-play-server clap-mml-realtime-play-server
```

### update

```
clap-mml-render-server update
```

or

```
clap-mml-realtime-play-server update
```

### Background:
- This project was forked from the original repository (clap-mml-render-tui) by cloning it. It retains the history up to the point of this split.

### Notes:
- The actual server / CLI / TUI functionalities are implemented on the `clap-mml-render-tui` side.
  - → Currently, the server process is being extracted to this project.