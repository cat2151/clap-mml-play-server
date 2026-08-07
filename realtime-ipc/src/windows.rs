use std::{
    mem::size_of,
    ptr,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Memory::{CreateFileMappingW, OpenFileMappingW, FILE_MAP_ALL_ACCESS, PAGE_READWRITE},
        SystemInformation::GetTickCount64,
        Threading::{GetCurrentProcessId, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE},
    },
};

use super::{
    validate_instance_id, FastIpcError, FastMidiCommand, FastMidiEvent, InstanceId, LimiterMeter,
    MAX_MIDI_MESSAGES, MAX_PATCH_BYTES, MAX_RESPONSE_BYTES,
};

mod command;
mod handles;
mod protocol;

use command::{pop_command, validate_midi_message, zeroed_slot};
use handles::*;
use protocol::*;

pub struct FastMidiServer {
    mapping: Mapping,
    command_event: OwnedHandle,
    response_event: OwnedHandle,
}

impl FastMidiServer {
    pub fn create(port: u16) -> Result<Self, FastIpcError> {
        let mapping_name = wide_name(port, "map");
        let command_event_name = wide_name(port, "command-event");
        let response_event_name = wide_name(port, "response-event");
        let mapping_handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                ptr::null(),
                PAGE_READWRITE,
                0,
                size_of::<SharedRing>() as u32,
                mapping_name.as_ptr(),
            )
        };
        if mapping_handle.is_null() {
            return Err(last_os_error("CreateFileMappingW"));
        }
        let mapping = map_handle(mapping_handle)?;
        let command_event = create_event(&command_event_name)?;
        let response_event = create_event(&response_event_name)?;

        unsafe {
            ptr::write_bytes(
                mapping.view.as_ptr().cast::<u8>(),
                0,
                size_of::<SharedRing>(),
            );
            let ring = mapping.view.as_ptr();
            (*ring)
                .server_pid
                .store(GetCurrentProcessId(), Ordering::Relaxed);
            (*ring)
                .heartbeat_ms
                .store(GetTickCount64(), Ordering::Relaxed);
            (*ring).version = VERSION;
            (*ring).magic = MAGIC;
        }

        Ok(Self {
            mapping,
            command_event,
            response_event,
        })
    }

    pub fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<FastMidiCommand>, FastIpcError> {
        let deadline = Instant::now() + timeout;
        self.touch_heartbeat();
        loop {
            if let Some(command) = pop_command(self.mapping.ring())? {
                return Ok(Some(command));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            let wait_ms = (deadline - now).as_millis().min(u32::MAX as u128) as u32;
            let wait = unsafe { WaitForSingleObject(self.command_event.0, wait_ms) };
            self.touch_heartbeat();
            match wait {
                // A command can be popped before its auto-reset event is consumed. In that
                // case the next wait observes a stale signal, so loop and check the ring again.
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => return Ok(None),
                _ => return Err(last_os_error("WaitForSingleObject")),
            }
        }
    }

    pub fn complete_request(
        &self,
        request_id: u32,
        result: Result<&[u8], &str>,
    ) -> Result<(), FastIpcError> {
        let (status, payload) = match result {
            Ok(payload) => (RESPONSE_OK, payload),
            Err(message) => (RESPONSE_ERROR, message.as_bytes()),
        };
        if payload.len() > MAX_RESPONSE_BYTES {
            return Err(FastIpcError::ResponseTooLong {
                bytes: payload.len(),
                max: MAX_RESPONSE_BYTES,
            });
        }
        let ring = self.mapping.ring();
        unsafe {
            let response = &mut *ring.response.get();
            response.request_id = request_id;
            response.status = status;
            response.payload_len = payload.len() as u32;
            response.payload[..payload.len()].copy_from_slice(payload);
        }
        ring.response_sequence.fetch_add(1, Ordering::Release);
        if unsafe { SetEvent(self.response_event.0) } == 0 {
            return Err(last_os_error("SetEvent"));
        }
        Ok(())
    }

    pub fn publish_limiter_meter(&self, meter: LimiterMeter) {
        let ring = self.mapping.ring();
        ring.limiter_current_bits
            .store(meter.current_reduction_db.to_bits(), Ordering::Release);
        update_atomic_max(&ring.limiter_peak_bits, meter.peak_reduction_db);
    }

    pub fn publish_underrun_frames(&self, frames: u64) {
        self.mapping
            .ring()
            .underrun_frames
            .store(frames, Ordering::Release);
    }

    fn touch_heartbeat(&self) {
        self.mapping
            .ring()
            .heartbeat_ms
            .store(unsafe { GetTickCount64() }, Ordering::Release);
    }
}

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
        self.patch_request(KIND_PREPARE_PATCH, instance_id, patch)
            .map(|_| ())
    }

    pub fn probe_patch(
        &mut self,
        instance_id: InstanceId,
        patch: Option<&str>,
    ) -> Result<Vec<u8>, FastIpcError> {
        self.patch_request(KIND_PROBE_PATCH, instance_id, patch)
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
        let ring = self.mapping.ring();
        LimiterMeter {
            current_reduction_db: f32::from_bits(ring.limiter_current_bits.load(Ordering::Acquire)),
            peak_reduction_db: f32::from_bits(ring.limiter_peak_bits.swap(0, Ordering::AcqRel)),
        }
    }

    pub fn underrun_frames(&self) -> u64 {
        self.mapping.ring().underrun_frames.load(Ordering::Acquire)
    }

    fn patch_request(
        &mut self,
        kind: u32,
        instance_id: InstanceId,
        patch: Option<&str>,
    ) -> Result<Vec<u8>, FastIpcError> {
        validate_instance_id(instance_id)?;
        let patch_bytes = patch.map(str::as_bytes).unwrap_or_default();
        if patch_bytes.len() > MAX_PATCH_BYTES {
            return Err(FastIpcError::PatchTooLong {
                bytes: patch_bytes.len(),
                max: MAX_PATCH_BYTES,
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
        self.push(slot)?;
        self.wait_for_response(request_id)
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

fn update_atomic_max(value: &AtomicU32, candidate: f32) {
    let mut current = value.load(Ordering::Acquire);
    loop {
        if f32::from_bits(current) >= candidate {
            return;
        }
        match value.compare_exchange_weak(
            current,
            candidate.to_bits(),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

fn validate_ring(ring: &SharedRing) -> Result<(), FastIpcError> {
    if ring.magic != MAGIC || ring.version != VERSION {
        return Err(FastIpcError::ProtocolMismatch);
    }
    Ok(())
}
