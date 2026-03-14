# clap-mml-render-tui

### Purpose

- For playing MML audio
- For casual installation. Only Rust is required.

### Technology Stack
- Plugin host library
  - https://github.com/prokopyl/clack

### Preparation

Please install [Surge XT](https://surge-synthesizer.github.io/).

```
winget install "Surge XT"
```

### Install

```
cargo install --force --git https://github.com/cat2151/clap-mml-render-tui
```

### Run

```
cmrt
```

You can input MML and play with it on the TUI screen.

### Server Mode

```
cmrt --server
```

- Interoperates with the bluesky-text-to-audio Chrome extension.
  - When an MML is found in a Bluesky post, it can be played with Surge XT.

# Breaking Changes
- Expect frequent breaking changes daily.

# Out of Scope
- Effects will likely require mandatory editing, so they are currently out of scope and postponed to a much later stage.