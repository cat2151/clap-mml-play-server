# clap-mml-play-server

### Usage
- Utilized as a library by `clap-mml-render-tui`.
- Utilized by `clap-mml-render-tui` as a separately launched server process.

### Realtime MIDI API

`clap-mml-realtime-play-server` accepts timestamp-free MIDI 1.0 channel voice messages. Messages
in one request are delivered in order at the start of the next audio chunk. The first request
switches playback to the plugin's Init Saw state.

```http
POST /midi
Content-Type: application/json

{"messages":[[128,60,0],[144,62,100]]}
```

Each message must contain exactly three bytes. Status bytes `0x80` through `0xEF` are accepted;
data bytes must be `0x00` through `0x7F`. Pad two-byte messages such as Program Change with a
zero third byte. SysEx, running status, and MIDI 2.0 are not supported by this endpoint.

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
