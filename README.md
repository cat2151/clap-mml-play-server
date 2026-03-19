# clap-mml-render-tui

### Usage

- For playing around with MML (Music Macro Language).
- Designed for casual installation. Only Rust is required.

### Technology Stack
- Plugin host library
  - https://github.com/prokopyl/clack

### Setup

Install [Surge XT](https://surge-synthesizer.github.io/).

```
winget install "Surge XT"
```

### Installation

``` 
cargo install --force --git https://github.com/cat2151/clap-mml-render-tui --package clap-mml-render-tui
```

### Running

```
cmrt
```

You can input MML and play with it in the TUI interface.

### Server Mode

```
cmrt --server
```

- Integrates with the bluesky-text-to-audio Chrome extension.
  - When a Bluesky post contains MML, it can be played using Surge XT.

# Breaking Changes
- Breaking changes are made frequently, on a daily basis.

# Future Plans
- Disable automatic updates via TOML configuration. In such cases, after quitting, a message like "An update is available. The update command is ~" will be displayed. Further automation is out of scope, as it has been verified that its complexity outweighs the benefits.

# Out of Scope
- Effects are likely essential for editing, but for now, they are explicitly out of scope and will be deferred to a much later stage.
