use anyhow::{Context as _, Result};
use cmrt_core::CoreConfig;
use serde::Deserialize;

pub(crate) const DEFAULT_REALTIME_PLAY_SERVER_PORT: u16 = 62154;
const REQUIRED_SAMPLE_RATE: f64 = 48_000.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RealtimeServerConfig {
    pub(crate) realtime_play_server_port: u16,
    pub(crate) patch_path: Option<String>,
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
        Self::from_toml_str(&text).with_context(|| {
            format!(
                "realtime play server 設定の読み込みに失敗 ({})",
                path.display()
            )
        })
    }

    fn from_toml_str(text: &str) -> Result<Self> {
        let overlay: RealtimeOverlayToml = toml::from_str(text)?;
        let config = Self {
            realtime_play_server_port: overlay.realtime_play_server_port,
            patch_path: normalize_patch_path(overlay.patch_path),
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.realtime_play_server_port == 0 {
            anyhow::bail!("realtime_play_server_port は 1〜65535 の範囲で設定してください");
        }
        Ok(())
    }
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
}
