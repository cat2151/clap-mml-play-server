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
        /// `"effects after instrument"` の値の JSON 文字列。空なら chain 無し。
        effect_chain: String,
        completion: std::sync::mpsc::SyncSender<std::result::Result<(), String>>,
    },
    /// 非演奏 bank への先読みロード。
    ///
    /// [`PlayerCommand::PrepareLivePatch`] と載せるものは同じだが、クライアントが
    /// 「この instance は鳴っている bank に属さない」と宣言している点が違う。
    /// coordinator はこれを根拠に対象 bank を render-disabled にし、**完了を待たずに**
    /// 演奏 bank を回し続ける。
    ///
    /// `completion` は **容量 1** で作ること（[`super::standby_completion_channel`]）。
    /// 0（rendezvous）にすると、完了を返す `send` が受け取り手を待って
    /// レンダーループごと止まる。受け取り手（IPC 受信スレッド）は poll するだけで、
    /// そこで待たないのが v10 の設計。
    PrepareStandbyLivePatch {
        generation: u64,
        instance_id: InstanceId,
        patch: Option<String>,
        effect_chain: String,
        completion: std::sync::mpsc::SyncSender<super::StandbyLoadResult>,
    },
    /// live instance 群の出力を `fade_frames` で 0 まで絞る。generation は上げない
    /// （上げるとリングの描画済み frame が捨てられ、fade の前に段差で切れる）。
    FadeOutInstances {
        instance_ids: Vec<InstanceId>,
        fade_frames: u32,
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

    /// timeline を張り直す。**live が既に走っているなら generation を上げない。**
    ///
    /// 上げると `start_generation()` がリング内の描画済みフレームを捨て、鳴っている音が
    /// 段差で 0 へ落ちたうえ、次の block が届くまで無音が挟まる。据え置けば前の演奏は
    /// ワーカー側の NoteOff の release のまま消え、新しい timeline はその続きから始まる。
    pub(super) fn submit_begin_live_timeline(
        &self,
        config: LiveTimelineConfig,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let generation = if state.live_requested {
            ensure_running(&state)?;
            // 同じ generation のままでも、出力を「再生中」へ戻す必要はある
            // （全 instance の停止で live を畳んだ後に張り直す場合）。
            audio_output.start_generation(state.generation);
            state.generation
        } else {
            begin_new_generation(&mut state, &audio_output)?
        };
        // 張り直しの前に届いた fadeout は、前の timeline の音に掛けるものなので残す。
        state
            .pending
            .retain(|command| matches!(command, PlayerCommand::FadeOutInstances { .. }));
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

    /// live instance 群の fadeout を積む。live が走っていなければ絞る音が無いので何もしない。
    pub(super) fn submit_fade_out_instances(
        &self,
        instance_ids: Vec<InstanceId>,
        fade_frames: u32,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        if !state.live_requested {
            return Ok(());
        }
        state.pending.push_back(PlayerCommand::FadeOutInstances {
            instance_ids,
            fade_frames,
        });
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
        effect_chain: String,
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
            effect_chain,
            completion,
        });
        self.command_available.notify_one();
        Ok(())
    }

    /// 先読みロードを積む。generation を上げない理由は
    /// [`Self::submit_prepare_live_patch`] と同じ（鳴っている bank の音を飛ばさない）。
    pub(super) fn submit_prepare_standby_live_patch(
        &self,
        instance_id: InstanceId,
        patch: Option<String>,
        effect_chain: String,
        completion: std::sync::mpsc::SyncSender<super::StandbyLoadResult>,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let generation = if state.live_requested {
            ensure_running(&state)?;
            state.generation
        } else {
            begin_new_generation(&mut state, &audio_output)?
        };
        state.live_requested = true;
        state
            .pending
            .push_back(PlayerCommand::PrepareStandbyLivePatch {
                generation,
                instance_id,
                patch,
                effect_chain,
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
