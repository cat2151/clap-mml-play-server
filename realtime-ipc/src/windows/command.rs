use std::{ptr, sync::atomic::Ordering};

use super::{
    protocol::{
        CommandSlot, SharedRing, KIND_BEGIN_LIVE_TIMELINE, KIND_MIDI, KIND_PREPARE_PATCH,
        KIND_PROBE_PATCH, KIND_SET_AUTO_GAIN, KIND_SET_BUFFER_MULTIPLIER, KIND_SET_INSTANCE_GAIN,
        KIND_SET_LIVE_TEMPO, KIND_STOP, KIND_STOP_ALL, KIND_TIMELINE_MIDI, SLOT_COUNT,
    },
    validate_instance_id, validate_ring, FastIpcError, FastMidiCommand, FastMidiEvent, InstanceId,
    LiveTempoChange, LiveTimelineConfig, TimelineMidiEvent, MAX_MIDI_MESSAGES, MAX_PATCH_BYTES,
};

/// instance ゲインの上限（千分率）。+12dB 相当までを許す。
const MAX_INSTANCE_GAIN_MILLI: u32 = 4_000;

pub(super) fn pop_command(ring: &SharedRing) -> Result<Option<FastMidiCommand>, FastIpcError> {
    validate_ring(ring)?;
    let read = ring.read_index.load(Ordering::Relaxed);
    let write = ring.write_index.load(Ordering::Acquire);
    if read == write {
        return Ok(None);
    }
    let index = (read as usize) % SLOT_COUNT;
    let slot = unsafe { ptr::read(ring.slots[index].get()) };
    ring.read_index
        .store(read.wrapping_add(1), Ordering::Release);
    decode_slot(slot).map(Some)
}

fn decode_slot(slot: CommandSlot) -> Result<FastMidiCommand, FastIpcError> {
    match slot.kind {
        KIND_STOP => Ok(FastMidiCommand::Stop {
            instance_id: decode_instance(slot.instance_id)?,
        }),
        KIND_STOP_ALL => Ok(FastMidiCommand::StopAll),
        KIND_SET_BUFFER_MULTIPLIER => {
            let multiplier = u16::try_from(slot.buffer_multiplier)
                .map_err(|_| FastIpcError::InvalidPayload("invalid buffer multiplier".into()))?;
            if !crate::is_valid_buffer_multiplier(multiplier) {
                return Err(FastIpcError::InvalidPayload(format!(
                    "buffer multiplier must be a power of two up to {}",
                    crate::MAX_BUFFER_MULTIPLIER
                )));
            }
            Ok(FastMidiCommand::SetBufferMultiplier { multiplier })
        }
        KIND_SET_INSTANCE_GAIN => {
            let gain_milli = slot.buffer_multiplier;
            if gain_milli > MAX_INSTANCE_GAIN_MILLI {
                return Err(FastIpcError::InvalidPayload(
                    "instance gain is out of range".into(),
                ));
            }
            Ok(FastMidiCommand::SetInstanceGain {
                instance_id: decode_instance(slot.instance_id)?,
                gain_milli,
            })
        }
        KIND_SET_AUTO_GAIN => match slot.buffer_multiplier {
            0 => Ok(FastMidiCommand::SetAutoGain { enabled: false }),
            1 => Ok(FastMidiCommand::SetAutoGain { enabled: true }),
            _ => Err(FastIpcError::InvalidPayload(
                "auto gain flag must be 0 or 1".into(),
            )),
        },
        KIND_PREPARE_PATCH | KIND_PROBE_PATCH => Ok(FastMidiCommand::PreparePatch {
            request_id: slot.request_id,
            instance_id: decode_instance(slot.instance_id)?,
            patch: decode_patch(&slot)?,
            probe: slot.kind == KIND_PROBE_PATCH,
        }),
        KIND_BEGIN_LIVE_TIMELINE => {
            let config = LiveTimelineConfig {
                timeline_id: slot.timeline_id,
                sample_rate_hz: f64::from_bits(slot.sample_rate_bits),
                tempo_bpm: f64::from_bits(slot.tempo_bits),
                time_signature_numerator: u16::try_from(slot.time_signature_numerator)
                    .map_err(|_| FastIpcError::InvalidPayload("invalid time signature".into()))?,
                time_signature_denominator: u16::try_from(slot.time_signature_denominator)
                    .map_err(|_| FastIpcError::InvalidPayload("invalid time signature".into()))?,
            };
            validate_timeline_config(config)?;
            Ok(FastMidiCommand::BeginLiveTimeline(config))
        }
        KIND_SET_LIVE_TEMPO => {
            let change = LiveTempoChange {
                timeline_id: slot.timeline_id,
                at_seconds: f64::from_bits(slot.timeline_seconds_bits[0]),
                tempo_bpm: f64::from_bits(slot.tempo_bits),
                time_signature_numerator: u16::try_from(slot.time_signature_numerator)
                    .map_err(|_| FastIpcError::InvalidPayload("invalid time signature".into()))?,
                time_signature_denominator: u16::try_from(slot.time_signature_denominator)
                    .map_err(|_| FastIpcError::InvalidPayload("invalid time signature".into()))?,
            };
            validate_tempo_change(change)?;
            Ok(FastMidiCommand::SetLiveTempo(change))
        }
        KIND_TIMELINE_MIDI => {
            let count = slot.message_count as usize;
            if count == 0 || count > MAX_MIDI_MESSAGES {
                return Err(FastIpcError::InvalidPayload("invalid event count".into()));
            }
            let mut events = Vec::with_capacity(count);
            for index in 0..count {
                let instance_id = slot.instance_ids[index];
                validate_instance_id(instance_id)?;
                let message = slot.messages[index];
                validate_midi_message(message)?;
                let timeline_seconds = f64::from_bits(slot.timeline_seconds_bits[index]);
                if !timeline_seconds.is_finite() || timeline_seconds < 0.0 {
                    return Err(FastIpcError::InvalidPayload(
                        "timeline seconds must be finite and non-negative".into(),
                    ));
                }
                events.push(TimelineMidiEvent {
                    timeline_id: slot.timeline_id,
                    instance_id,
                    timeline_seconds,
                    message,
                });
            }
            Ok(FastMidiCommand::TimelineMidi { events })
        }
        KIND_MIDI => {
            let count = slot.message_count as usize;
            if count == 0 || count > MAX_MIDI_MESSAGES {
                return Err(FastIpcError::InvalidPayload("invalid event count".into()));
            }
            let mut events = Vec::with_capacity(count);
            for index in 0..count {
                let instance_id = slot.instance_ids[index];
                validate_instance_id(instance_id)?;
                let message = slot.messages[index];
                validate_midi_message(message)?;
                events.push(FastMidiEvent {
                    instance_id,
                    offset_frames: slot.offsets[index],
                    message,
                });
            }
            Ok(FastMidiCommand::Midi { events })
        }
        _ => Err(FastIpcError::InvalidPayload("unknown command kind".into())),
    }
}

/// テンポ変化点の検証。[`validate_timeline_config`] と同じ条件に「変化点の絶対秒が
/// 有限・非負」を足したもの。送信側 ([`super::timeline`]) と受信側で同じものを通す。
pub(super) fn validate_tempo_change(change: LiveTempoChange) -> Result<(), FastIpcError> {
    if change.timeline_id == 0
        || !change.at_seconds.is_finite()
        || change.at_seconds < 0.0
        || !change.tempo_bpm.is_finite()
        || change.tempo_bpm <= 0.0
        || change.time_signature_numerator == 0
        || change.time_signature_denominator == 0
        || !change.time_signature_denominator.is_power_of_two()
    {
        return Err(FastIpcError::InvalidPayload(
            "invalid live tempo change".into(),
        ));
    }
    Ok(())
}

fn validate_timeline_config(config: LiveTimelineConfig) -> Result<(), FastIpcError> {
    if config.timeline_id == 0
        || !config.sample_rate_hz.is_finite()
        || config.sample_rate_hz <= 0.0
        || !config.tempo_bpm.is_finite()
        || config.tempo_bpm <= 0.0
        || config.time_signature_numerator == 0
        || config.time_signature_denominator == 0
        || !config.time_signature_denominator.is_power_of_two()
    {
        return Err(FastIpcError::InvalidPayload(
            "invalid live timeline configuration".into(),
        ));
    }
    Ok(())
}

fn decode_patch(slot: &CommandSlot) -> Result<Option<String>, FastIpcError> {
    let patch_len = slot.patch_len as usize;
    if patch_len > MAX_PATCH_BYTES {
        return Err(FastIpcError::InvalidPayload(
            "patch payload length is invalid".into(),
        ));
    }
    if slot.has_patch == 0 {
        return Ok(None);
    }
    Ok(Some(
        std::str::from_utf8(&slot.patch[..patch_len])
            .map_err(|_| FastIpcError::InvalidPayload("patch is not UTF-8".into()))?
            .to_string(),
    ))
}

fn decode_instance(raw: u32) -> Result<InstanceId, FastIpcError> {
    let instance_id = u8::try_from(raw)
        .map_err(|_| FastIpcError::InvalidPayload("instance id is invalid".into()))?;
    validate_instance_id(instance_id)?;
    Ok(instance_id)
}

pub(super) fn validate_midi_message(message: [u8; 3]) -> Result<(), FastIpcError> {
    if !(0x80..=0xef).contains(&message[0]) || message[1] > 0x7f || message[2] > 0x7f {
        return Err(FastIpcError::InvalidPayload(format!(
            "invalid MIDI channel voice message: [{}, {}, {}]",
            message[0], message[1], message[2]
        )));
    }
    Ok(())
}

pub(super) fn zeroed_slot() -> CommandSlot {
    unsafe { std::mem::zeroed() }
}
