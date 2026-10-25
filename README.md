# clap-mml-play-server

### Usage
- Used as a library by `clap-mml-render-tui`.
- Launched and used as a server process by `clap-mml-render-tui`.

### install

```
cargo install --force --git https://github.com/cat2151/clap-mml-play-server clap-mml-realtime-play-server
```

### self update

Regardless of which server you run it from, it updates both `clap-mml-render-server` and `clap-mml-realtime-play-server` together.

```
clap-mml-render-server update
```

Or

```
clap-mml-realtime-play-server update
```

To compare the build-time commit hash with the remote `main` branch's commit hash, use the `check` subcommand for each command.

```
clap-mml-render-server check
clap-mml-realtime-play-server check
```

### Patch Pitch Tracking Measurement

This allows measuring whether changing the note of a single patch causes its pitch to track the keyboard (`tracks_keyboard`). Any plugin can be used. No audio device is opened. Bulk measurement of all patches in the drum/percussion section is performed by `clap-mml-render-tui`'s `cmrt build-pitch-tracking`, which launches this subcommand for each patch.

```powershell
cargo run --release -p clap-mml-realtime-play-server -- probe-pitch-tracking --patch "patches_factory/Percussion/Drum One.fxp" --json
```

Refer to [Patch Pitch Tracking Measurement](docs/patch-pitch-tracking.md) for details on judgment criteria, limitations, and the process leading to shared data (`cat-music-patterns/patch-pitch-tracking.json`).

### Background:
- This project was branched off (forked) from the original repository (`clap-mml-render-tui`) and preserves the history up to the point of the split.

### Notes:
- Actual server / CLI / TUI functionalities are currently implemented on the `clap-mml-render-tui` side.
  - → The server process is currently being extracted to this repository.