use std::{
    cell::UnsafeCell,
    ffi::c_void,
    mem::size_of,
    ptr::{self, NonNull},
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, HANDLE, INVALID_HANDLE_VALUE,
        STILL_ACTIVE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::{
        Memory::{
            CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
            FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
        },
        SystemInformation::GetTickCount64,
        Threading::{
            CreateEventW, GetCurrentProcessId, GetExitCodeProcess, OpenEventW, OpenProcess,
            SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

use super::{FastIpcError, FastMidiCommand, MAX_MIDI_MESSAGES, MAX_PATCH_BYTES};

const MAGIC: [u8; 8] = *b"CMRTMIDI";
const VERSION: u32 = 2;
const SLOT_COUNT: usize = 64;
const KIND_MIDI: u32 = 1;
const KIND_STOP: u32 = 2;
const KIND_SET_BUFFER_MULTIPLIER: u32 = 3;
const SERVER_STALE_MS: u64 = 1_000;

#[repr(C)]
struct CommandSlot {
    kind: u32,
    message_count: u32,
    patch_len: u32,
    has_patch: u32,
    buffer_multiplier: u32,
    messages: [[u8; 3]; MAX_MIDI_MESSAGES],
    patch: [u8; MAX_PATCH_BYTES],
}

#[repr(C, align(64))]
struct SharedRing {
    magic: [u8; 8],
    version: u32,
    _reserved: u32,
    server_pid: AtomicU32,
    client_pid: AtomicU32,
    write_index: AtomicU32,
    read_index: AtomicU32,
    heartbeat_ms: AtomicU64,
    slots: [UnsafeCell<CommandSlot>; SLOT_COUNT],
}

unsafe impl Sync for SharedRing {}

struct Mapping {
    handle: HANDLE,
    view: NonNull<SharedRing>,
}

unsafe impl Send for Mapping {}

impl Mapping {
    fn ring(&self) -> &SharedRing {
        // SAFETY: the mapping is valid for this object's lifetime. Shared mutations use atomics
        // and the SPSC ownership protocol.
        unsafe { self.view.as_ref() }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: both handles were created by the matching Win32 APIs and are owned here.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.view.as_ptr().cast::<c_void>(),
            });
            CloseHandle(self.handle);
        }
    }
}

struct OwnedHandle(HANDLE);

unsafe impl Send for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this object owns the handle.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub struct FastMidiServer {
    mapping: Mapping,
    event: OwnedHandle,
}

impl FastMidiServer {
    pub fn create(port: u16) -> Result<Self, FastIpcError> {
        let mapping_name = wide_name(port, "map");
        let event_name = wide_name(port, "event");
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
        let event_handle = unsafe { CreateEventW(ptr::null(), 0, 0, event_name.as_ptr()) };
        if event_handle.is_null() {
            return Err(last_os_error("CreateEventW"));
        }
        let event = OwnedHandle(event_handle);

        // SAFETY: the server exclusively initializes the mapping before publishing magic/version.
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

        Ok(Self { mapping, event })
    }

    pub fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<FastMidiCommand>, FastIpcError> {
        self.touch_heartbeat();
        if let Some(command) = pop_command(self.mapping.ring())? {
            return Ok(Some(command));
        }
        let wait_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let wait = unsafe { WaitForSingleObject(self.event.0, wait_ms) };
        self.touch_heartbeat();
        match wait {
            WAIT_OBJECT_0 | WAIT_TIMEOUT => pop_command(self.mapping.ring()),
            _ => Err(last_os_error("WaitForSingleObject")),
        }
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
    event: OwnedHandle,
    pid: u32,
}

impl FastMidiClient {
    pub fn connect(port: u16) -> Result<Self, FastIpcError> {
        let mapping_name = wide_name(port, "map");
        let event_name = wide_name(port, "event");
        let mapping_handle =
            unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, mapping_name.as_ptr()) };
        if mapping_handle.is_null() {
            return Err(FastIpcError::NotAvailable);
        }
        let mapping = map_handle(mapping_handle)?;
        validate_ring(mapping.ring())?;
        let event_handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr()) };
        if event_handle.is_null() {
            return Err(FastIpcError::NotAvailable);
        }
        let event = OwnedHandle(event_handle);
        let pid = unsafe { GetCurrentProcessId() };
        claim_client(mapping.ring(), pid)?;
        let write = mapping.ring().write_index.load(Ordering::Acquire);
        mapping.ring().read_index.store(write, Ordering::Release);
        Ok(Self {
            mapping,
            event,
            pid,
        })
    }

    pub fn send_midi(
        &mut self,
        messages: &[[u8; 3]],
        patch: Option<&str>,
    ) -> Result<(), FastIpcError> {
        if messages.is_empty() {
            return Err(FastIpcError::InvalidPayload(
                "messages must not be empty".into(),
            ));
        }
        if messages.len() > MAX_MIDI_MESSAGES {
            return Err(FastIpcError::TooManyMidiMessages {
                count: messages.len(),
                max: MAX_MIDI_MESSAGES,
            });
        }
        validate_midi_messages(messages)?;
        let patch_bytes = patch.map(str::as_bytes).unwrap_or_default();
        if patch_bytes.len() > MAX_PATCH_BYTES {
            return Err(FastIpcError::PatchTooLong {
                bytes: patch_bytes.len(),
                max: MAX_PATCH_BYTES,
            });
        }
        let mut slot = zeroed_slot();
        slot.kind = KIND_MIDI;
        slot.message_count = messages.len() as u32;
        slot.messages[..messages.len()].copy_from_slice(messages);
        if patch.is_some() {
            slot.has_patch = 1;
            slot.patch_len = patch_bytes.len() as u32;
            slot.patch[..patch_bytes.len()].copy_from_slice(patch_bytes);
        }
        self.push(slot)
    }

    pub fn stop(&mut self) -> Result<(), FastIpcError> {
        let mut slot = zeroed_slot();
        slot.kind = KIND_STOP;
        self.push(slot)
    }

    pub fn set_buffer_multiplier(&mut self, multiplier: u8) -> Result<(), FastIpcError> {
        if !matches!(multiplier, 1 | 2 | 4 | 8) {
            return Err(FastIpcError::InvalidPayload(
                "buffer multiplier must be 1, 2, 4, or 8".into(),
            ));
        }
        let mut slot = zeroed_slot();
        slot.kind = KIND_SET_BUFFER_MULTIPLIER;
        slot.buffer_multiplier = u32::from(multiplier);
        self.push(slot)
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
        // SAFETY: the client is the sole producer and release publishes the complete slot.
        unsafe {
            ptr::write(ring.slots[index].get(), slot);
        }
        ring.write_index
            .store(write.wrapping_add(1), Ordering::Release);
        if unsafe { SetEvent(self.event.0) } == 0 {
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

fn pop_command(ring: &SharedRing) -> Result<Option<FastMidiCommand>, FastIpcError> {
    validate_ring(ring)?;
    let read = ring.read_index.load(Ordering::Relaxed);
    let write = ring.write_index.load(Ordering::Acquire);
    if read == write {
        return Ok(None);
    }
    let index = (read as usize) % SLOT_COUNT;
    // SAFETY: the server is the sole consumer and acquire observes the published slot.
    let slot = unsafe { ptr::read(ring.slots[index].get()) };
    ring.read_index
        .store(read.wrapping_add(1), Ordering::Release);
    decode_slot(slot).map(Some)
}

fn decode_slot(slot: CommandSlot) -> Result<FastMidiCommand, FastIpcError> {
    match slot.kind {
        KIND_STOP => Ok(FastMidiCommand::Stop),
        KIND_SET_BUFFER_MULTIPLIER => {
            let multiplier = u8::try_from(slot.buffer_multiplier)
                .map_err(|_| FastIpcError::InvalidPayload("invalid buffer multiplier".into()))?;
            if !matches!(multiplier, 1 | 2 | 4 | 8) {
                return Err(FastIpcError::InvalidPayload(
                    "buffer multiplier must be 1, 2, 4, or 8".into(),
                ));
            }
            Ok(FastMidiCommand::SetBufferMultiplier { multiplier })
        }
        KIND_MIDI => {
            let count = slot.message_count as usize;
            let patch_len = slot.patch_len as usize;
            if count == 0 || count > MAX_MIDI_MESSAGES || patch_len > MAX_PATCH_BYTES {
                return Err(FastIpcError::InvalidPayload("invalid field length".into()));
            }
            let patch = if slot.has_patch == 0 {
                None
            } else {
                Some(
                    std::str::from_utf8(&slot.patch[..patch_len])
                        .map_err(|_| FastIpcError::InvalidPayload("patch is not UTF-8".into()))?
                        .to_string(),
                )
            };
            validate_midi_messages(&slot.messages[..count])?;
            Ok(FastMidiCommand::Midi {
                messages: slot.messages[..count].to_vec(),
                patch,
            })
        }
        _ => Err(FastIpcError::InvalidPayload("unknown command kind".into())),
    }
}

fn validate_midi_messages(messages: &[[u8; 3]]) -> Result<(), FastIpcError> {
    if let Some(message) = messages.iter().find(|message| {
        !(0x80..=0xef).contains(&message[0]) || message[1] > 0x7f || message[2] > 0x7f
    }) {
        return Err(FastIpcError::InvalidPayload(format!(
            "invalid MIDI channel voice message: [{}, {}, {}]",
            message[0], message[1], message[2]
        )));
    }
    Ok(())
}

fn zeroed_slot() -> CommandSlot {
    // SAFETY: CommandSlot contains only integer and byte-array fields, for which zero is valid.
    unsafe { std::mem::zeroed() }
}

fn validate_ring(ring: &SharedRing) -> Result<(), FastIpcError> {
    if ring.magic != MAGIC || ring.version != VERSION {
        return Err(FastIpcError::ProtocolMismatch);
    }
    Ok(())
}

fn claim_client(ring: &SharedRing, pid: u32) -> Result<(), FastIpcError> {
    loop {
        let owner = ring.client_pid.load(Ordering::Acquire);
        if owner == 0 {
            if ring
                .client_pid
                .compare_exchange(0, pid, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(());
            }
            continue;
        }
        if process_is_running(owner) {
            return Err(FastIpcError::AlreadyConnected);
        }
        let _ = ring
            .client_pid
            .compare_exchange(owner, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

fn process_is_running(pid: u32) -> bool {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return unsafe { GetLastError() } != ERROR_INVALID_PARAMETER;
    }
    let mut exit_code = 0;
    let read_ok = unsafe { GetExitCodeProcess(process, &mut exit_code) } != 0;
    unsafe { CloseHandle(process) };
    read_ok && exit_code == STILL_ACTIVE as u32
}

fn map_handle(handle: HANDLE) -> Result<Mapping, FastIpcError> {
    let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, size_of::<SharedRing>()) };
    let Some(view) = NonNull::new(view.Value.cast::<SharedRing>()) else {
        unsafe { CloseHandle(handle) };
        return Err(last_os_error("MapViewOfFile"));
    };
    Ok(Mapping { handle, view })
}

fn wide_name(port: u16, suffix: &str) -> Vec<u16> {
    format!("Local\\cmrt-realtime-midi-v{VERSION}-{port}-{suffix}\0")
        .encode_utf16()
        .collect()
}

fn last_os_error(operation: &'static str) -> FastIpcError {
    FastIpcError::Os {
        operation,
        code: unsafe { GetLastError() },
    }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
