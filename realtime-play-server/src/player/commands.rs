//! ワーカースレッドへ渡すコマンドと、その投入キュー。
//!
//! `RealtimePlayer`（`super`）が公開 API の顔をし、ここは「命令をどう並べ、
//! どの世代（generation）に属させるか」だけを持つ。世代はレンダー済みの音声を
//! 捨ててよいかの判定に使われる（`audio_output` 参照）。

use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
};

use anyhow::Result;
use cmrt_core::{RealtimePlaybackSchedule, VoicingReport};
use cmrt_realtime_ipc::{
    FastMidiEvent, InstanceId, LiveTempoChange, LiveTimelineConfig, TimelineMidiEvent,
};

use super::audio_output::AudioOutputControl;

pub(super) struct PlayerInner {
    pub(super) state: Mutex<PlayerState>,
    command_available: Condvar,
}

#[derive(Default)]
pub(super) struct PlayerState {
    generation: u64,
    pending: VecDeque<PlayerCommand>,
    live_requested: bool,
    live_timeline_id: Option<u64>,
    shutdown: bool,
}

#[derive(Debug)]
pub(super) enum PlayerCommand {
    Play {
        generation: u64,
        schedule: RealtimePlaybackSchedule,
        patch: Option<String>,
    },
    StopAll {
        generation: u64,
    },
    StopInstance {
        generation: u64,
        instance_id: InstanceId,
    },
    Midi {
        generation: u64,
        events: Vec<FastMidiEvent>,
        enter_live: bool,
    },
    BeginLiveTimeline {
        generation: u64,
        config: LiveTimelineConfig,
    },
    SetLiveTempo {
        generation: u64,
        change: LiveTempoChange,
    },
    TimelineMidi {
        generation: u64,
        events: Vec<TimelineMidiEvent>,
    },
    PrepareLivePatch {
        generation: u64,
        instance_id: InstanceId,
        patch: Option<String>,
        completion: std::sync::mpsc::SyncSender<std::result::Result<(), String>>,
    },
    ProbeLivePatch {
        generation: u64,
        instance_id: InstanceId,
        patch: Option<String>,
        completion: std::sync::mpsc::SyncSender<std::result::Result<VoicingReport, String>>,
    },
}

impl Default for PlayerInner {
    fn default() -> Self {
        Self {
            state: Mutex::new(PlayerState::default()),
            command_available: Condvar::new(),
        }
    }
}

impl PlayerInner {
    pub(super) fn submit_play(
        &self,
        schedule: RealtimePlaybackSchedule,
        patch: Option<String>,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let generation = begin_new_generation(&mut state, &audio_output)?;
        state.live_requested = false;
        state.live_timeline_id = None;
        state.pending.clear();
        state.pending.push_back(PlayerCommand::Play {
            generation,
            schedule,
            patch,
        });
        self.command_available.notify_one();
        Ok(())
    }

    pub(super) fn submit_stop(&self, audio_output: Arc<AudioOutputControl>) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        state.generation = next_generation(state.generation);
        let generation = state.generation;
        audio_output.stop_generation(generation);
        state.live_requested = false;
        state.live_timeline_id = None;
        state.pending.clear();
        state
            .pending
            .push_back(PlayerCommand::StopAll { generation });
        self.command_available.notify_one();
        Ok(())
    }

    pub(super) fn submit_stop_instance(
        &self,
        instance_id: InstanceId,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let generation = begin_new_generation(&mut state, &audio_output)?;
        state.pending.push_back(PlayerCommand::StopInstance {
            generation,
            instance_id,
        });
        self.command_available.notify_one();
        Ok(())
    }

    pub(super) fn submit_midi(
        &self,
        events: Vec<FastMidiEvent>,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        let enter_live = !state.live_requested;
        if enter_live {
            state.generation = next_generation(state.generation);
            state.pending.clear();
            state.live_requested = true;
            audio_output.start_generation(state.generation);
        }
        let generation = state.generation;
        state.pending.push_back(PlayerCommand::Midi {
            generation,
            events,
            enter_live,
        });
        self.command_available.notify_one();
        Ok(())
    }

    pub(super) fn submit_begin_live_timeline(
        &self,
        config: LiveTimelineConfig,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let generation = begin_new_generation(&mut state, &audio_output)?;
        state.pending.clear();
        state.live_requested = true;
        state.live_timeline_id = Some(config.timeline_id);
        state
            .pending
            .push_back(PlayerCommand::BeginLiveTimeline { generation, config });
        self.command_available.notify_one();
        Ok(())
    }

    /// tempo map への追記。**generation を上げないこと。**
    ///
    /// 上げると `start_generation()` がリング内の描画済みフレームを捨て、テンポを
    /// 変えただけで音が飛ぶ。テンポ変化はタイムライン上のデータであって、
    /// 音楽的な epoch の切り替えではない。
    pub(super) fn submit_set_live_tempo(&self, change: LiveTempoChange) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        let Some(active_id) = state.live_timeline_id else {
            anyhow::bail!("no live timeline has been started");
        };
        if change.timeline_id != active_id {
            anyhow::bail!("timeline mismatch: active={active_id}");
        }
        let generation = state.generation;
        state
            .pending
            .push_back(PlayerCommand::SetLiveTempo { generation, change });
        self.command_available.notify_one();
        Ok(())
    }

    pub(super) fn submit_timeline_midi(&self, events: Vec<TimelineMidiEvent>) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        let Some(active_id) = state.live_timeline_id else {
            anyhow::bail!("no live timeline has been started");
        };
        if events.iter().any(|event| event.timeline_id != active_id) {
            anyhow::bail!("timeline mismatch: active={active_id}");
        }
        let generation = state.generation;
        state
            .pending
            .push_back(PlayerCommand::TimelineMidi { generation, events });
        self.command_available.notify_one();
        Ok(())
    }

    pub(super) fn submit_prepare_live_patch(
        &self,
        instance_id: InstanceId,
        patch: Option<String>,
        completion: std::sync::mpsc::SyncSender<std::result::Result<(), String>>,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        // live が既に走っているなら generation を上げない。上げると `start_generation()` が
        // リング内の描画済みフレームを全部捨てさせるため、鳴っていない instance へ patch を
        // 積むだけで鳴っている instance の音が飛ぶ。ワーカー側の `PrepareLivePatch` は
        // `renderers[index]` と `instances[index]` しか触らないので、据え置きで安全。
        // grid sequencer の chord mode はこれを使い、演奏の裏で次の bank を仕込む。
        let generation = if state.live_requested {
            ensure_running(&state)?;
            state.generation
        } else {
            begin_new_generation(&mut state, &audio_output)?
        };
        state.live_requested = true;
        state.pending.push_back(PlayerCommand::PrepareLivePatch {
            generation,
            instance_id,
            patch,
            completion,
        });
        self.command_available.notify_one();
        Ok(())
    }

    pub(super) fn submit_probe_live_patch(
        &self,
        instance_id: InstanceId,
        patch: Option<String>,
        completion: std::sync::mpsc::SyncSender<std::result::Result<VoicingReport, String>>,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let generation = begin_new_generation(&mut state, &audio_output)?;
        state.live_requested = true;
        state.pending.push_back(PlayerCommand::ProbeLivePatch {
            generation,
            instance_id,
            patch,
            completion,
        });
        self.command_available.notify_one();
        Ok(())
    }

    pub(super) fn wait_for_command(&self) -> Option<PlayerCommand> {
        let mut state = self.state.lock().unwrap();
        while state.pending.is_empty() && !state.shutdown {
            state = self.command_available.wait(state).unwrap();
        }
        if state.shutdown {
            return None;
        }
        state.pending.pop_front()
    }

    pub(super) fn pop_pending_command(&self) -> Option<PlayerCommand> {
        let mut state = self.state.lock().unwrap();
        if state.shutdown {
            return None;
        }
        state.pending.pop_front()
    }

    pub(super) fn shutdown(&self, audio_output: &AudioOutputControl) {
        let mut state = self.state.lock().unwrap();
        state.shutdown = true;
        self.command_available.notify_one();
        audio_output.shutdown();
    }
}

pub(super) fn begin_new_generation(
    state: &mut PlayerState,
    audio_output: &AudioOutputControl,
) -> Result<u64> {
    ensure_running(state)?;
    state.generation = next_generation(state.generation);
    audio_output.start_generation(state.generation);
    Ok(state.generation)
}

pub(super) fn ensure_running(state: &PlayerState) -> Result<()> {
    if state.shutdown {
        anyhow::bail!("realtime play worker is stopped");
    }
    Ok(())
}

fn next_generation(current: u64) -> u64 {
    current.wrapping_add(1).max(1)
}

#[cfg(test)]
mod tests;
