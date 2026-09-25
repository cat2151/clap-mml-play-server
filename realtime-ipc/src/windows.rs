use std::{ptr, sync::atomic::Ordering, time::Instant};

use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Memory::{OpenFileMappingW, FILE_MAP_ALL_ACCESS},
        SystemInformation::GetTickCount64,
        Threading::{GetCurrentProcessId, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE},
    },
};

use super::{
    validate_instance_id, FastIpcError, FastMidiCommand, FastMidiEvent, InstanceId, LimiterMeter,
    LiveTempoChange, LiveTimelineConfig, TimelineMidiEvent, TimingMetrics, MAX_EFFECT_CHAIN_BYTES,
    MAX_FADE_OUT_MS, MAX_INSTANCE_COUNT, MAX_MIDI_MESSAGES, MAX_PATCH_BYTES, MAX_RESPONSE_BYTES,
    MAX_STANDBY_ERROR_BYTES,
};

mod command;
mod handles;
mod meters;
mod protocol;
mod server;
mod standby;
mod timeline;

use command::{validate_fade_out, validate_midi_message, validate_tempo_change, zeroed_slot};
use handles::*;
use protocol::*;
pub use server::FastMidiServer;

pub struct FastMidiClient {
    mapping: Mapping,
    command_event: OwnedHandle,
    response_event: OwnedHandle,
    pid: u32,
    next_request_id: u32,
}

impl FastMidiClient {
    pub fn connect(port: u16) -> Result<Self, FastIpcError> {
        let mapping_name = wide_name(port, "map");
        let command_event_name = wide_name(port, "command-event");
        let response_event_name = wide_name(port, "response-event");
        let mapping_handle =
            unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, mapping_name.as_ptr()) };
        if mapping_handle.is_null() {
            return Err(FastIpcError::NotAvailable);
        }
        let mapping = map_handle(mapping_handle)?;
        validate_ring(mapping.ring())?;
        let command_event = open_event(&command_event_name, EVENT_MODIFY_STATE)?;
        let response_event = open_event(&response_event_name, SYNCHRONIZE_ACCESS)?;
        let pid = unsafe { GetCurrentProcessId() };
        claim_client(mapping.ring(), pid)?;
        let write = mapping.ring().write_index.load(Ordering::Acquire);
        mapping.ring().read_index.store(write, Ordering::Release);
        Ok(Self {
            mapping,
            command_event,
            response_event,
            pid,
            next_request_id: 1,
        })
    }

    pub fn send_events(&mut self, events: &[FastMidiEvent]) -> Result<(), FastIpcError> {
        if events.is_empty() {
            return Err(FastIpcError::InvalidPayload(
                "events must not be empty".into(),
            ));
        }
        if events.len() > MAX_MIDI_MESSAGES {
            return Err(FastIpcError::TooManyMidiMessages {
                count: events.len(),
                max: MAX_MIDI_MESSAGES,
            });
        }
        for event in events {
            validate_instance_id(event.instance_id)?;
            validate_midi_message(event.message)?;
        }
        let mut slot = zeroed_slot();
        slot.kind = KIND_MIDI;
        slot.message_count = events.len() as u32;
        for (index, event) in events.iter().enumerate() {
            slot.messages[index] = event.message;
            slot.offsets[index] = event.offset_frames;
            slot.instance_ids[index] = event.instance_id;
        }
        self.push(slot)
    }

    pub fn prepare_patch(
        &mut self,
        instance_id: InstanceId,
        patch: Option<&str>,
    ) -> Result<(), FastIpcError> {
        self.prepare_patch_with_effect_chain(instance_id, patch, "")
    }

    /// 音色と、その instance の出力に掛ける effect chain を同じ時点で差し替える。
    ///
    /// `effect_chain` は MML 先頭 JSON の `"effects after instrument"` の値を JSON 文字列に
    /// したもの。空なら chain を外す（[`Self::prepare_patch`] と同じ）。
    pub fn prepare_patch_with_effect_chain(
        &mut self,
        instance_id: InstanceId,
        patch: Option<&str>,
        effect_chain: &str,
    ) -> Result<(), FastIpcError> {
        self.patch_request(KIND_PREPARE_PATCH, instance_id, patch, effect_chain)
            .map(|_| ())
    }

    /// 非演奏 bank へ音色を先読みする。
    ///
    /// **protocol v10 以降、ここで待つのは「サーバーが要求を受け付けた」までで、
    /// ロードの完了ではない。** ロード結果は [`Self::poll_standby_completion`] で
    /// 別 slot から拾う。完了まで待ちたい呼び出し元は
    /// [`Self::begin_standby_patch`] で request ID を取ってからポーリングすること。
    ///
    /// 「対象 instance が演奏していない bank にある」という宣言を伴うので、
    /// 現在 bank の行音色変更や MML overlay には使わないこと。
    pub fn prepare_standby_patch(
        &mut self,
        instance_id: InstanceId,
        patch: Option<&str>,
    ) -> Result<(), FastIpcError> {
        self.begin_standby_patch(instance_id, patch).map(|_| ())
    }

    /// 先読みを要求し、受付応答まで待って request ID を返す。
    ///
    /// 完了は返らない。呼び出し元は **要求の前に** [`Self::standby_watermark`] を
    /// 読み、返った request ID と組で [`Self::poll_standby_completion`] を回す。
    pub fn begin_standby_patch(
        &mut self,
        instance_id: InstanceId,
        patch: Option<&str>,
    ) -> Result<u32, FastIpcError> {
        self.begin_standby_patch_with_effect_chain(instance_id, patch, "")
    }

    /// [`Self::begin_standby_patch`] に effect chain を同梱する形
    /// （chain の意味は [`Self::prepare_patch_with_effect_chain`] と同じ）。
    pub fn begin_standby_patch_with_effect_chain(
        &mut self,
        instance_id: InstanceId,
        patch: Option<&str>,
        effect_chain: &str,
    ) -> Result<u32, FastIpcError> {
        self.patch_request_with_id(KIND_PREPARE_STANDBY_PATCH, instance_id, patch, effect_chain)
            .map(|(request_id, _)| request_id)
    }

    pub fn probe_patch(
        &mut self,
        instance_id: InstanceId,
        patch: Option<&str>,
    ) -> Result<Vec<u8>, FastIpcError> {
        self.patch_request(KIND_PROBE_PATCH, instance_id, patch, "")
    }

    pub fn stop(&mut self, instance_id: InstanceId) -> Result<(), FastIpcError> {
        validate_instance_id(instance_id)?;
        let mut slot = zeroed_slot();
        slot.kind = KIND_STOP;
        slot.instance_id = u32::from(instance_id);
        self.push(slot)
    }

    pub fn stop_all(&mut self) -> Result<(), FastIpcError> {
        let mut slot = zeroed_slot();
        slot.kind = KIND_STOP_ALL;
        self.push(slot)
    }

    /// 指定した live instance 群を、今の音量から 0 まで `fade_ms` ミリ秒で絞る。応答は待たない。
    ///
    /// 0 に達した instance は鳴っている voice と effect chain の余韻を捨てる。
    pub fn fade_out_instances(
        &mut self,
        instance_ids: &[InstanceId],
        fade_ms: u32,
    ) -> Result<(), FastIpcError> {
        validate_fade_out(instance_ids, fade_ms)?;
        let mut slot = zeroed_slot();
        slot.kind = KIND_FADE_OUT_INSTANCES;
        slot.message_count = instance_ids.len() as u32;
        slot.instance_ids[..instance_ids.len()].copy_from_slice(instance_ids);
        slot.buffer_multiplier = fade_ms;
        self.push(slot)
    }

    pub fn set_buffer_multiplier(&mut self, multiplier: u16) -> Result<(), FastIpcError> {
        if !crate::is_valid_buffer_multiplier(multiplier) {
            return Err(FastIpcError::InvalidPayload(format!(
                "buffer multiplier must be a power of two up to {}",
                crate::MAX_BUFFER_MULTIPLIER
            )));
        }
        let mut slot = zeroed_slot();
        slot.kind = KIND_SET_BUFFER_MULTIPLIER;
        slot.buffer_multiplier = u32::from(multiplier);
        self.push(slot)
    }

    pub fn set_auto_gain_enabled(&mut self, enabled: bool) -> Result<(), FastIpcError> {
        let mut slot = zeroed_slot();
        slot.kind = KIND_SET_AUTO_GAIN;
        slot.buffer_multiplier = u32::from(enabled);
        self.push(slot)
    }

    pub fn limiter_meter(&self) -> LimiterMeter {
        meters::limiter_meter(self.mapping.ring())
    }

    pub fn underrun_frames(&self) -> u64 {
        meters::underrun_frames(self.mapping.ring())
    }

    pub fn auto_gain_db(&self) -> [f32; MAX_INSTANCE_COUNT] {
        meters::auto_gain_db(self.mapping.ring())
    }

    pub fn timing_metrics(&self) -> TimingMetrics {
        meters::timing_metrics(self.mapping.ring())
    }

    /// これから出す standby request の基準 sequence。request 送信の **前** に読む。
    ///
    /// 完了通知はこの値より後に publish されたものだけを自分のものとして扱う。
    /// request ID が wrap しても古い完了を成功と取り違えないための番人。
    pub fn standby_watermark(&self) -> u64 {
        standby::standby_watermark(self.mapping.ring())
    }

    /// standby 完了通知を非 blocking に読む。`None` はまだ完了していない。
    pub fn poll_standby_completion(
        &self,
        request_id: u32,
        since_sequence: u64,
    ) -> Option<Result<(), FastIpcError>> {
        standby::read_standby_completion(self.mapping.ring(), request_id, since_sequence)
    }

    fn patch_request(
        &mut self,
        kind: u32,
        instance_id: InstanceId,
        patch: Option<&str>,
        effect_chain: &str,
    ) -> Result<Vec<u8>, FastIpcError> {
        self.patch_request_with_id(kind, instance_id, patch, effect_chain)
            .map(|(_, payload)| payload)
    }

    /// patch 系要求を 1 件出して、汎用応答（= 受付応答）まで待つ。
    ///
    /// request ID も返すのは、standby のように「受付」と「完了」が別 slot へ
    /// 分かれた要求で、完了通知の突き合わせに ID が要るため。
    fn patch_request_with_id(
        &mut self,
        kind: u32,
        instance_id: InstanceId,
        patch: Option<&str>,
        effect_chain: &str,
    ) -> Result<(u32, Vec<u8>), FastIpcError> {
        validate_instance_id(instance_id)?;
        let patch_bytes = patch.map(str::as_bytes).unwrap_or_default();
        if patch_bytes.len() > MAX_PATCH_BYTES {
            return Err(FastIpcError::PatchTooLong {
                bytes: patch_bytes.len(),
                max: MAX_PATCH_BYTES,
            });
        }
        let chain_bytes = effect_chain.as_bytes();
        if chain_bytes.len() > MAX_EFFECT_CHAIN_BYTES {
            return Err(FastIpcError::EffectChainTooLong {
                bytes: chain_bytes.len(),
                max: MAX_EFFECT_CHAIN_BYTES,
            });
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        let mut slot = zeroed_slot();
        slot.kind = kind;
        slot.request_id = request_id;
        slot.instance_id = u32::from(instance_id);
        if patch.is_some() {
            slot.has_patch = 1;
            slot.patch_len = patch_bytes.len() as u32;
            slot.patch[..patch_bytes.len()].copy_from_slice(patch_bytes);
        }
        slot.effect_chain_len = chain_bytes.len() as u32;
        slot.effect_chain[..chain_bytes.len()].copy_from_slice(chain_bytes);
        self.push(slot)?;
        let payload = self.wait_for_response(request_id)?;
        Ok((request_id, payload))
    }

    fn wait_for_response(&self, request_id: u32) -> Result<Vec<u8>, FastIpcError> {
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        let mut observed = self
            .mapping
            .ring()
            .response_sequence
            .load(Ordering::Acquire);
        loop {
            if let Some(response) = self.read_response(request_id, observed)? {
                return response;
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(FastIpcError::ResponseTimeout);
            }
            let wait_ms = (deadline - now).as_millis().min(u32::MAX as u128) as u32;
            let wait = unsafe { WaitForSingleObject(self.response_event.0, wait_ms) };
            match wait {
                WAIT_OBJECT_0 => {
                    observed = self
                        .mapping
                        .ring()
                        .response_sequence
                        .load(Ordering::Acquire);
                }
                WAIT_TIMEOUT => return Err(FastIpcError::ResponseTimeout),
                _ => return Err(last_os_error("WaitForSingleObject")),
            }
        }
    }

    fn read_response(
        &self,
        request_id: u32,
        sequence: u32,
    ) -> Result<Option<Result<Vec<u8>, FastIpcError>>, FastIpcError> {
        if sequence == 0 {
            return Ok(None);
        }
        let response = unsafe { &*self.mapping.ring().response.get() };
        if response.request_id != request_id {
            return Ok(None);
        }
        let len = response.payload_len as usize;
        if len > MAX_RESPONSE_BYTES {
            return Err(FastIpcError::InvalidPayload(
                "response payload length is invalid".into(),
            ));
        }
        let payload = response.payload[..len].to_vec();
        let result = match response.status {
            RESPONSE_OK => Ok(payload),
            RESPONSE_ERROR => Err(FastIpcError::RequestFailed(
                String::from_utf8_lossy(&payload).into_owned(),
            )),
            _ => {
                return Err(FastIpcError::InvalidPayload(
                    "response status is invalid".into(),
                ))
            }
        };
        Ok(Some(result))
    }

    fn push(&mut self, slot: CommandSlot) -> Result<(), FastIpcError> {
        validate_ring(self.mapping.ring())?;
        if self.mapping.ring().client_pid.load(Ordering::Acquire) != self.pid {
            return Err(FastIpcError::ServerStopped);
        }
        let now = unsafe { GetTickCount64() };
        let heartbeat = self.mapping.ring().heartbeat_ms.load(Ordering::Acquire);
        if now.saturating_sub(heartbeat) > SERVER_STALE_MS {
            return Err(FastIpcError::ServerStopped);
        }
        let ring = self.mapping.ring();
        let write = ring.write_index.load(Ordering::Relaxed);
        let read = ring.read_index.load(Ordering::Acquire);
        if write.wrapping_sub(read) >= SLOT_COUNT as u32 {
            return Err(FastIpcError::QueueFull);
        }
        let index = (write as usize) % SLOT_COUNT;
        unsafe { ptr::write(ring.slots[index].get(), slot) };
        ring.write_index
            .store(write.wrapping_add(1), Ordering::Release);
        if unsafe { SetEvent(self.command_event.0) } == 0 {
            return Err(last_os_error("SetEvent"));
        }
        Ok(())
    }
}

impl Drop for FastMidiClient {
    fn drop(&mut self) {
        let _ = self.mapping.ring().client_pid.compare_exchange(
            self.pid,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

fn validate_ring(ring: &SharedRing) -> Result<(), FastIpcError> {
    if ring.magic != MAGIC || ring.version != VERSION {
        return Err(FastIpcError::ProtocolMismatch);
    }
    Ok(())
}
