use anyhow::{Context as _, Result};
use cmrt_core::CoreConfig;
use serde::Deserialize;

pub(crate) const DEFAULT_REALTIME_PLAY_SERVER_PORT: u16 = 62154;
pub(crate) const DEFAULT_LIVE_INSTANCE_COUNT: usize = 16;
pub(crate) const LIVE_INSTANCE_COUNT_ENV: &str = "CMRT_LIVE_INSTANCE_COUNT";
/// grid sequencer の chord mode は N トラックを 2 bank（= 2N instance）へ割り当てるため、
/// トラック数の 2 倍まで許す。上限は `cmrt_realtime_ipc::MAX_INSTANCE_COUNT`。
///
/// 6 は 3 トラック（chord / bass / アルペジオ）用、14 は 7 トラック（その 3 つに
/// drum 4 role を足したもの）用。クライアント側の
/// `realtime-play/src/lib.rs` の `SUPPORTED_SERVER_INSTANCE_COUNTS` と必ず揃えること。
pub(crate) const SUPPORTED_LIVE_INSTANCE_COUNTS: [usize; 8] = [1, 2, 4, 6, 8, 14, 16, 32];
const REQUIRED_SAMPLE_RATE: f64 = 48_000.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RealtimeServerConfig {
    pub(crate) realtime_play_server_port: u16,
    pub(crate) patch_path: Option<String>,
    pub(crate) live_instance_count: usize,
}

#[derive(Debug, Deserialize)]
struct RealtimeOverlayToml {
    #[serde(default = "default_realtime_play_server_port")]
    realtime_play_server_port: u16,
    #[serde(default)]
    patch_path: Option<String>,
}

impl RealtimeServerConfig {
    pub(crate) fn load() -> Result<Self> {
        let path = cmrt_runtime::config_file_path().ok_or_else(|| {
            anyhow::anyhow!(
                "システムの設定ディレクトリが取得できません。HOME 環境変数などを確認してください。"
            )
        })?;
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("config.toml が読めない ({})", path.display()))?;
        let mut config = Self::from_toml_str(&text).with_context(|| {
            format!(
                "realtime play server 設定の読み込みに失敗 ({})",
                path.display()
            )
        })?;
        config.live_instance_count = live_instance_count_from_env()?;
        config.validate()?;
        Ok(config)
    }

    fn from_toml_str(text: &str) -> Result<Self> {
        let overlay: RealtimeOverlayToml = toml::from_str(text)?;
        let config = Self {
            realtime_play_server_port: overlay.realtime_play_server_port,
            patch_path: normalize_patch_path(overlay.patch_path),
            live_instance_count: DEFAULT_LIVE_INSTANCE_COUNT,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.realtime_play_server_port == 0 {
            anyhow::bail!("realtime_play_server_port は 1〜65535 の範囲で設定してください");
        }
        if !SUPPORTED_LIVE_INSTANCE_COUNTS.contains(&self.live_instance_count) {
            anyhow::bail!("{LIVE_INSTANCE_COUNT_ENV} は {SUPPORTED_LIVE_INSTANCE_COUNTS:?} のいずれかにしてください");
        }
        Ok(())
    }
}

fn live_instance_count_from_env() -> Result<usize> {
    match std::env::var(LIVE_INSTANCE_COUNT_ENV) {
        Ok(value) => parse_live_instance_count(&value),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_LIVE_INSTANCE_COUNT),
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("{LIVE_INSTANCE_COUNT_ENV} がUTF-8ではありません")
        }
    }
}

fn parse_live_instance_count(value: &str) -> Result<usize> {
    let count = value.trim().parse::<usize>().with_context(|| {
        format!("{LIVE_INSTANCE_COUNT_ENV} は整数で指定してください（現在値: {value:?}）")
    })?;
    if !SUPPORTED_LIVE_INSTANCE_COUNTS.contains(&count) {
        anyhow::bail!("{LIVE_INSTANCE_COUNT_ENV} は {SUPPORTED_LIVE_INSTANCE_COUNTS:?} のいずれかにしてください");
    }
    Ok(count)
}

pub(crate) fn validate_realtime_play_server_config(
    cfg: &cmrt_runtime::Config,
    realtime_cfg: &RealtimeServerConfig,
) -> Result<()> {
    if cfg.plugin_path.trim().is_empty() {
        anyhow::bail!("plugin_path が空です");
    }
    if cfg.sample_rate != REQUIRED_SAMPLE_RATE {
        anyhow::bail!("realtime play server は sample_rate = 48000 の config のみ対応します");
    }
    realtime_cfg.validate()
}

pub(crate) fn core_config_from_runtime(
    cfg: &cmrt_runtime::Config,
    realtime_cfg: &RealtimeServerConfig,
) -> CoreConfig {
    CoreConfig {
        plugin_id: cfg.plugin_id.clone(),
        output_midi: cfg.output_midi.clone(),
        output_wav: cfg.output_wav.clone(),
        sample_rate: cfg.sample_rate,
        buffer_size: cfg.buffer_size,
        patch_path: realtime_cfg.patch_path.clone(),
        patches_dir: cmrt_runtime::core_config_patch_root_dir(cfg),
        random_patch: false,
    }
}

fn default_realtime_play_server_port() -> u16 {
    DEFAULT_REALTIME_PLAY_SERVER_PORT
}

fn normalize_patch_path(patch_path: Option<String>) -> Option<String> {
    patch_path.and_then(|path| {
        let path = path.trim();
        (!path.is_empty()).then(|| path.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_defaults_to_realtime_play_server_port() {
        let config = RealtimeServerConfig::from_toml_str("").unwrap();

        assert_eq!(
            config.realtime_play_server_port,
            DEFAULT_REALTIME_PLAY_SERVER_PORT
        );
        assert_eq!(config.patch_path, None);
        assert_eq!(config.live_instance_count, DEFAULT_LIVE_INSTANCE_COUNT);
    }

    #[test]
    fn overlay_reads_realtime_play_server_port_and_patch_path() {
        let config = RealtimeServerConfig::from_toml_str(
            r#"
realtime_play_server_port = 62222
patch_path = "Pads/Pad 1.fxp"
"#,
        )
        .unwrap();

        assert_eq!(config.realtime_play_server_port, 62222);
        assert_eq!(config.patch_path.as_deref(), Some("Pads/Pad 1.fxp"));
    }

    #[test]
    fn overlay_rejects_zero_port() {
        let error =
            RealtimeServerConfig::from_toml_str("realtime_play_server_port = 0").unwrap_err();

        assert!(error.to_string().contains("realtime_play_server_port"));
    }

    #[test]
    fn overlay_treats_blank_patch_path_as_none() {
        let config = RealtimeServerConfig::from_toml_str(
            r#"
patch_path = "   "
"#,
        )
        .unwrap();

        assert_eq!(config.patch_path, None);
    }

    #[test]
    fn live_instance_count_accepts_supported_values() {
        for count in SUPPORTED_LIVE_INSTANCE_COUNTS {
            assert_eq!(
                parse_live_instance_count(&count.to_string()).unwrap(),
                count
            );
        }
    }

    #[test]
    fn live_instance_count_rejects_unsupported_values() {
        for value in ["0", "3", "5", "17", "33", "64", "not-a-number"] {
            assert!(parse_live_instance_count(value).is_err(), "{value}");
        }
    }

    /// `Config` は別 repo（cmrt-runtime）の型なので、構造体リテラルで書くと
    /// あちらでフィールドが 1 つ増えるだけでこのサーバーがビルド不能になる。
    /// 増えるフィールドには serde default が付く決まりなので、TOML から作って追従不要にする。
    fn runtime_config(extra: &str) -> cmrt_runtime::Config {
        let base = r#"
plugin_path = "plugin.clap"
input_midi = "input.mid"
output_midi = "output.mid"
output_wav = "output.wav"
sample_rate = 48000
buffer_size = 512
"#;
        toml::from_str(&format!("{base}{extra}")).unwrap()
    }

    /// `plugin_id` を CoreConfig まで運べないと、descriptor を複数持つ CLAP で
    /// 起動ログとライブ instance の descriptor 選択が食い違う。
    #[test]
    fn core_config_from_runtime_carries_plugin_id() {
        let cfg = runtime_config(
            "plugin_id = \"com.digital-suburban.dexed\"
",
        );
        let realtime_cfg = RealtimeServerConfig::from_toml_str("").unwrap();

        let core_cfg = core_config_from_runtime(&cfg, &realtime_cfg);

        assert_eq!(
            core_cfg.plugin_id.as_deref(),
            Some("com.digital-suburban.dexed")
        );
    }

    #[test]
    fn core_config_from_runtime_leaves_plugin_id_unset_when_config_omits_it() {
        let cfg = runtime_config("");
        let realtime_cfg = RealtimeServerConfig::from_toml_str("").unwrap();

        let core_cfg = core_config_from_runtime(&cfg, &realtime_cfg);

        assert_eq!(core_cfg.plugin_id, None);
    }

    /// 共有メモリプロトコルが表現できない数を設定で許してしまうと、
    /// instance_id の検証（`validate_instance_id`）で初めて弾かれることになる。
    #[test]
    fn every_supported_count_fits_in_the_shared_memory_protocol() {
        for count in SUPPORTED_LIVE_INSTANCE_COUNTS {
            assert!(
                count <= cmrt_realtime_ipc::MAX_INSTANCE_COUNT,
                "{count} は MAX_INSTANCE_COUNT を超えている"
            );
        }
    }
}
