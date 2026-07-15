# clap-mml-play-server

### Usage
- Utilized as a library by `clap-mml-render-tui`.
- Utilized by `clap-mml-render-tui` as a separately launched server process.

### Install

```
cargo install --force --git https://github.com/cat2151/clap-mml-play-server clap-mml-realtime-play-server
```

### Self Update

Running the update command from either server will update both `clap-mml-render-server` and `clap-mml-realtime-play-server` simultaneously.

```
clap-mml-render-server update
```

or

```
clap-mml-realtime-play-server update
```

To compare the commit hash at build time with the commit hash of the remote main branch, use the `check` subcommand for each command.

```
clap-mml-render-server check
clap-mml-realtime-play-server check
```

### Background:
- This project was forked from the original repository (`clap-mml-render-tui`). It carries the commit history up to the point of the fork.

### Note:
- The actual server / CLI / TUI functionalities are implemented within `clap-mml-render-tui`.
  - → The server process is currently being extracted into this repository.