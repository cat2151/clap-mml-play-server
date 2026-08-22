//! コマンドライン引数の解釈。
//!
//! サブコマンドが増えても `main` が太らないよう、`clap` の定義と `CliAction` への
//! 変換だけをここへ置く。実処理は `main` か [`crate::probe`] にある。

use anyhow::Result;
use clap::{error::ErrorKind, Parser, Subcommand, ValueEnum};
use cmrt_core::PatchVoicing;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CliAction {
    Run,
    Update,
    Check,
    ProbeVoicing {
        patch: String,
        previous_patch: Option<String>,
        json: bool,
        expect: Option<ExpectedVoicing>,
    },
    ProbeCapabilities {
        plugin_path: Option<String>,
        plugin_id: Option<String>,
        json: bool,
    },
    PrintHelp(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ExpectedVoicing {
    Mono,
    Poly,
    Unknown,
}

impl From<ExpectedVoicing> for PatchVoicing {
    fn from(value: ExpectedVoicing) -> Self {
        match value {
            ExpectedVoicing::Mono => Self::Mono,
            ExpectedVoicing::Poly => Self::Poly,
            ExpectedVoicing::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "clap-mml-realtime-play-server",
    about = "Play Standard MIDI Files through a CLAP plugin",
    disable_help_subcommand = true,
    disable_version_flag = true,
    args_conflicts_with_subcommands = true,
    after_help = "CONFIG:\n    config_local_dir()/clap-mml-render-tui/config.toml\n\nHTTP:\n    GET /health\n    POST /play               request: Standard MIDI File bytes, Content-Type: audio/midi | audio/x-midi | application/octet-stream\n    POST /play-mml           request: MML text (leading {\"Surge XT patch\": ...} JSON selects the patch), Content-Type: text/plain\n    POST /stop\n\nLIVE MIDI (Windows):\n    1/2/4/8/16/32 CLAP instances through named shared memory on the realtime server port\n    CMRT_LIVE_INSTANCE_COUNT selects the count (default: 16)\n    32 is for the grid sequencer chord mode, which double-buffers 16 tracks across two banks"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Stop running workspace binaries and reinstall them
    Update,
    /// Compare the embedded commit hash with the remote main branch
    Check,
    /// Probe a patch for mono/poly voicing without opening an audio device
    ProbeVoicing {
        /// Patch path relative to patches_dir, or an absolute path
        #[arg(long)]
        patch: String,
        /// Probe this patch first on the same plugin instance
        #[arg(long)]
        previous_patch: Option<String>,
        /// Print the complete report as JSON
        #[arg(long)]
        json: bool,
        /// Fail unless the final decision matches this value
        #[arg(long, value_enum)]
        expect: Option<ExpectedVoicing>,
    },
    /// Measure a CLAP plugin's descriptors, ports, factories and extensions
    ProbeCapabilities {
        /// CLAP to measure. Defaults to every plugin the config can reach
        #[arg(long)]
        plugin_path: Option<String>,
        /// Descriptor ID to select, for CLAPs that expose more than one
        #[arg(long)]
        plugin_id: Option<String>,
        /// Print the complete report as JSON
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn parse_cli<I, T>(args: I) -> Result<CliAction>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    match Cli::try_parse_from(args) {
        Ok(cli) => match cli.command {
            Some(Commands::Update) => Ok(CliAction::Update),
            Some(Commands::Check) => Ok(CliAction::Check),
            Some(Commands::ProbeVoicing {
                patch,
                previous_patch,
                json,
                expect,
            }) => Ok(CliAction::ProbeVoicing {
                patch,
                previous_patch,
                json,
                expect,
            }),
            Some(Commands::ProbeCapabilities {
                plugin_path,
                plugin_id,
                json,
            }) => Ok(CliAction::ProbeCapabilities {
                plugin_path,
                plugin_id,
                json,
            }),
            None => Ok(CliAction::Run),
        },
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            Ok(CliAction::PrintHelp(error.to_string()))
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests;
