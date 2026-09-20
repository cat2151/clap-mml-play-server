//! factory preset を [`EffectRenderer`] へ載せる。plugin ごとに手順が違う。

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use super::renderer::EffectRenderer;
use super::rms_dbfs;
use crate::surge_fx_preset::{
    calibration_state_xml, juce_xml, param_layout, parse_srgfx, parse_state_xml,
    snapshot_state_xml, SurgeFxParamRanges, SurgeFxSnapshot, SurgeFxStateReport,
    SURGE_FX_PLUGIN_ID,
};
use crate::tone3000_preset::{
    parse_t3k_preset, tone3000_state_blob, Tone3000Preset, TONE3000_PLUGIN_ID,
};

/// TONE3000 の model が載って音が出るまでの待ち上限。
const TONE3000_AUDIBLE_TIMEOUT: Duration = Duration::from_secs(20);
/// この RMS を超えたら「音が出た」とみなす。
const AUDIBLE_DBFS: f32 = -60.0;
const PROBE_TONE_HZ: f64 = 220.0;
const PROBE_TONE_AMPLITUDE: f64 = 0.25;

impl EffectRenderer {
    fn ensure_plugin(&self, expected: &str) -> Result<()> {
        if self.plugin_id() != expected {
            bail!(
                "preset は '{expected}' 用だが、載っている effect は '{}'",
                self.plugin_id()
            );
        }
        Ok(())
    }

    /// factory preset ファイルを載せる。形式は載っている plugin で決まる。
    ///
    /// Surge XT Effects は `.srgfx` の先頭 snapshot、TONE3000 は `.t3kpreset`
    /// （`activePresetId` はファイル名の uuid）。
    pub fn load_preset_file(&mut self, path: &Path) -> Result<()> {
        match self.plugin_id() {
            SURGE_FX_PLUGIN_ID => {
                let xml = std::fs::read_to_string(path)
                    .with_context(|| format!("{} が読めない", path.display()))?;
                let snapshot = parse_srgfx(&xml)
                    .with_context(|| path.display().to_string())?
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("{} に snapshot が無い", path.display()))?;
                self.load_surge_fx_snapshot(&snapshot)
            }
            TONE3000_PLUGIN_ID => {
                let bytes = std::fs::read(path)
                    .with_context(|| format!("{} が読めない", path.display()))?;
                let preset =
                    parse_t3k_preset(&bytes).with_context(|| path.display().to_string())?;
                let preset_id = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .ok_or_else(|| anyhow::anyhow!("{} の名前が取れない", path.display()))?;
                self.load_tone3000_preset(&preset, preset_id).map(|_| ())
            }
            other => bail!("plugin '{other}' の preset 形式を知らない"),
        }
    }

    /// state を読ませて、plugin が保存し直した自己申告を返す。
    pub fn surge_fx_self_report(&mut self, xml: &str) -> Result<SurgeFxStateReport> {
        self.load_state(&juce_xml::encode(xml))?;
        let saved = self.save_state()?;
        parse_state_xml(&juce_xml::decode(&saved)?)
    }

    /// 正規化 0 / 1 に対応する実値を、plugin の自己申告から測る。
    pub fn measure_surge_fx_ranges(&mut self, fx_type: i32) -> Result<SurgeFxParamRanges> {
        let at_zero = self.surge_fx_self_report(&calibration_state_xml(fx_type, 0.0))?;
        let at_one = self.surge_fx_self_report(&calibration_state_xml(fx_type, 1.0))?;
        SurgeFxParamRanges::from_reports(&at_zero, &at_one)
    }

    /// `.srgfx` の snapshot を載せる。range 計測のため state の load/save を 2 回はさむ。
    pub fn load_surge_fx_snapshot(&mut self, snapshot: &SurgeFxSnapshot) -> Result<()> {
        self.ensure_plugin(SURGE_FX_PLUGIN_ID)?;
        let layout = param_layout(snapshot.fx_type).ok_or_else(|| {
            anyhow::anyhow!(
                "Surge XT Effects の type {} ('{}') は未対応",
                snapshot.fx_type,
                snapshot.name
            )
        })?;
        let ranges = self.measure_surge_fx_ranges(snapshot.fx_type)?;
        let xml = snapshot_state_xml(snapshot, layout, &ranges)?;
        self.load_state(&juce_xml::encode(&xml))
            .with_context(|| format!("Surge XT Effects preset '{}' の load", snapshot.name))
    }

    /// `.t3kpreset` を載せ、model が読み込まれて音が出るまで待つ。
    ///
    /// TONE3000 は state load 後に model を非同期で読み、その間は無音を出す。
    /// readiness を知る API が無いので、正弦波を通して出力で確かめる。戻るときは
    /// `reset()` 済みで、probe の残響は残らない。
    pub fn load_tone3000_preset(
        &mut self,
        preset: &Tone3000Preset,
        preset_id: &str,
    ) -> Result<Duration> {
        self.ensure_plugin(TONE3000_PLUGIN_ID)?;
        let blob = tone3000_state_blob(self.init_state(), preset, preset_id)?;
        self.load_state(&blob)
            .with_context(|| format!("TONE3000 preset '{}' の load", preset.name))?;
        self.ensure_active()?;
        let waited = self
            .wait_until_audible()
            .with_context(|| format!("TONE3000 preset '{}'", preset.name))?;
        self.reset();
        Ok(waited)
    }

    /// 正弦波を 1 block ずつ通し、出力が無音でなくなるまでの時間を返す。
    pub fn wait_until_audible(&mut self) -> Result<Duration> {
        let started = Instant::now();
        let frames = self.buf_size();
        let sample_rate = self.sample_rate().get();
        let mut probe = vec![0.0_f32; frames * 2];
        let mut phase_index: u64 = 0;
        loop {
            self.pump_main_thread();
            for (index, frame) in probe.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let t = (phase_index + index as u64) as f64 / sample_rate;
                let sample = (PROBE_TONE_AMPLITUDE
                    * (2.0 * std::f64::consts::PI * PROBE_TONE_HZ * t).sin())
                    as f32;
                frame[0] = sample;
                frame[1] = sample;
            }
            phase_index += frames as u64;
            self.process(&mut probe, None)?;
            if rms_dbfs(&probe) > AUDIBLE_DBFS {
                return Ok(started.elapsed());
            }
            if started.elapsed() > TONE3000_AUDIBLE_TIMEOUT {
                bail!(
                    "{:?} 待っても effect の出力が無音",
                    TONE3000_AUDIBLE_TIMEOUT
                );
            }
        }
    }
}
