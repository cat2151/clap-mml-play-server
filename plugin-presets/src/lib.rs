//! CLAP の preset-discovery を持たないプラグインについて、host が代わりに受け持つ音色の知識。
//!
//! プラグインごとの module に「何を 1 件とするか」と「1 件をどう解釈し、何を送るバイト列にするか」を置く。
//! CLAP で実際に送る層は持たない（`clack` に依存しないため）。
//! 背景は `docs/adr/0006-no-generic-clap-preset-api.md`。

pub mod dexed;
pub mod dragonfly;
pub mod effect_preset;
pub mod floe;
pub mod juce_value_tree;
mod lexical_path;
pub mod patch_list;
pub mod sforzando;
pub mod shu;
pub mod surge_fx;
pub mod tone3000;
pub mod tyrelln6;
pub mod vaporizer2;
pub mod voyage_voyage;

pub use lexical_path::lexical_absolute;
