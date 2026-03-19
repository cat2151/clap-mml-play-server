# clap-mml-render-tui

### Usage

- For playing around with sound using MML.
- Easy to install; only requires Rust.

### Technology Stack
- Plugin host library
  - https://github.com/prokopyl/clack

### Prerequisites

Please install [Surge XT](https://surge-synthesizer.github.io/).

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

You can experiment with sounds by inputting MML in the TUI screen.

### Server Mode

```
cmrt --server
```

- Integrates with the bluesky-text-to-audio Chrome extension.
  - When MML is found in a Bluesky post, it can be played using Surge XT.

# Breaking Changes
- Frequent daily breaking changes are introduced.

# Roadmap
- Option to disable automatic updates via TOML; in that case, upon quitting, an "update available" message will display the update command. Further automation has been determined to be outside the scope due to its complexity outweighing its benefits.

# Out of Scope
- Effects are likely to require editing, so for now, they are considered out of scope and will be deferred to a much later stage.