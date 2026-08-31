//! 共有メモリ IPC のサーバー側（play server が使う）。
//!
//! mapping と event を作り、コマンドリングから読み、応答・メーター・standby 完了を
//! 公開する。クライアント側（`FastMidiClient`）は親 module の `windows.rs` にある。

use std::{
    mem::size_of,
    ptr,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Memory::{CreateFileMappingW, PAGE_READWRITE},
        SystemInformation::GetTickCount64,
        Threading::{GetCurrentProcessId, SetEvent, WaitForSingleObject},
    },
};

use super::{
    command::pop_command,
    handles::{create_event, last_os_error, map_handle, wide_name, Mapping, OwnedHandle},
    meters,
    protocol::{SharedRing, MAGIC, RESPONSE_ERROR, RESPONSE_OK, VERSION},
    standby, FastIpcError, FastMidiCommand, LimiterMeter, TimingMetrics, MAX_RESPONSE_BYTES,
};

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

    /// standby patch load の **完了** を通知する。
    ///
    /// [`Self::complete_request`] が返すのは受付応答であって完了ではない。
    /// ロードは秒単位かかることがあるので、その間も受信ループが次のコマンドを
    /// 読めるように、完了だけを専用 slot へ分離してある。戻り値は確定した
    /// seqlock sequence。
    ///
    /// response event も叩くが、これは「何か変わった」ヒントに過ぎない。
    /// クライアントは [`FastMidiClient::poll_standby_completion`] で
    /// 非 blocking にポーリングする前提であり、この event を取り逃しても壊れない。
    pub fn publish_standby_completion(
        &self,
        request_id: u32,
        result: Result<(), &str>,
    ) -> Result<u64, FastIpcError> {
        let sequence = standby::publish_standby_completion(self.mapping.ring(), request_id, result);
        if unsafe { SetEvent(self.response_event.0) } == 0 {
            return Err(last_os_error("SetEvent"));
        }
        Ok(sequence)
    }

    pub fn publish_limiter_meter(&self, meter: LimiterMeter) {
        meters::publish_limiter_meter(self.mapping.ring(), meter);
    }

    pub fn publish_underrun_frames(&self, frames: u64) {
        meters::publish_underrun_frames(self.mapping.ring(), frames);
    }

    pub fn publish_auto_gain_db(&self, gains_db: &[f32]) {
        meters::publish_auto_gain_db(self.mapping.ring(), gains_db);
    }

    pub fn publish_timing_metrics(&self, metrics: TimingMetrics) {
        meters::publish_timing_metrics(self.mapping.ring(), metrics);
    }

    fn touch_heartbeat(&self) {
        self.mapping
            .ring()
            .heartbeat_ms
            .store(unsafe { GetTickCount64() }, Ordering::Release);
    }
}
