use super::*;
use std::time::Duration;

pub struct FastMidiClient;

impl FastMidiClient {
    pub fn connect(_port: u16) -> Result<Self, FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn send_events(&mut self, _events: &[FastMidiEvent]) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn begin_live_timeline(&mut self, _config: LiveTimelineConfig) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn set_live_tempo(&mut self, _change: LiveTempoChange) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn send_timeline_events(
        &mut self,
        _events: &[TimelineMidiEvent],
    ) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn prepare_patch(
        &mut self,
        _instance_id: InstanceId,
        _patch: Option<&str>,
    ) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn prepare_patch_with_effect_chain(
        &mut self,
        _instance_id: InstanceId,
        _patch: Option<&str>,
        _effect_chain: &str,
    ) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn prepare_standby_patch(
        &mut self,
        _instance_id: InstanceId,
        _patch: Option<&str>,
    ) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn begin_standby_patch(
        &mut self,
        _instance_id: InstanceId,
        _patch: Option<&str>,
    ) -> Result<u32, FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn begin_standby_patch_with_effect_chain(
        &mut self,
        _instance_id: InstanceId,
        _patch: Option<&str>,
        _effect_chain: &str,
    ) -> Result<u32, FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn probe_patch(
        &mut self,
        _instance_id: InstanceId,
        _patch: Option<&str>,
    ) -> Result<Vec<u8>, FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn stop(&mut self, _instance_id: InstanceId) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn stop_all(&mut self) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn fade_out_instances(
        &mut self,
        _instance_ids: &[InstanceId],
        _fade_ms: u32,
    ) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn set_buffer_multiplier(&mut self, _multiplier: u16) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn set_auto_gain_enabled(&mut self, _enabled: bool) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn limiter_meter(&self) -> LimiterMeter {
        LimiterMeter::default()
    }

    pub fn underrun_frames(&self) -> u64 {
        0
    }

    pub fn auto_gain_db(&self) -> [f32; MAX_INSTANCE_COUNT] {
        [0.0; MAX_INSTANCE_COUNT]
    }

    pub fn timing_metrics(&self) -> TimingMetrics {
        TimingMetrics::default()
    }

    pub fn standby_watermark(&self) -> u64 {
        0
    }

    pub fn poll_standby_completion(
        &self,
        _request_id: u32,
        _since_sequence: u64,
    ) -> Option<Result<(), FastIpcError>> {
        Some(Err(FastIpcError::UnsupportedPlatform))
    }
}

pub struct FastMidiServer;

impl FastMidiServer {
    pub fn create(_port: u16) -> Result<Self, FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn recv_timeout(
        &mut self,
        _timeout: Duration,
    ) -> Result<Option<FastMidiCommand>, FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn complete_request(
        &self,
        _request_id: u32,
        _result: Result<&[u8], &str>,
    ) -> Result<(), FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }

    pub fn publish_limiter_meter(&self, _meter: LimiterMeter) {}

    pub fn publish_underrun_frames(&self, _frames: u64) {}

    pub fn publish_auto_gain_db(&self, _gains_db: &[f32]) {}

    pub fn publish_timing_metrics(&self, _metrics: TimingMetrics) {}

    pub fn publish_standby_completion(
        &self,
        _request_id: u32,
        _result: Result<(), &str>,
    ) -> Result<u64, FastIpcError> {
        Err(FastIpcError::UnsupportedPlatform)
    }
}
