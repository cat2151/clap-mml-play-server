use super::*;

impl FastMidiClient {
    pub fn begin_live_timeline(&mut self, config: LiveTimelineConfig) -> Result<(), FastIpcError> {
        validate_timeline_config(config)?;
        let mut slot = zeroed_slot();
        slot.kind = KIND_BEGIN_LIVE_TIMELINE;
        slot.timeline_id = config.timeline_id;
        slot.sample_rate_bits = config.sample_rate_hz.to_bits();
        slot.tempo_bits = config.tempo_bpm.to_bits();
        slot.time_signature_numerator = u32::from(config.time_signature_numerator);
        slot.time_signature_denominator = u32::from(config.time_signature_denominator);
        self.push(slot)
    }

    /// 走っている live timeline の tempo map へ「`at_seconds` から新しいテンポ」を積む。
    ///
    /// [`Self::begin_live_timeline`] と違い timeline は作り直されないので、演奏も
    /// プラグインの状態も途切れない。
    pub fn set_live_tempo(&mut self, change: LiveTempoChange) -> Result<(), FastIpcError> {
        validate_tempo_change(change)?;
        let mut slot = zeroed_slot();
        slot.kind = KIND_SET_LIVE_TEMPO;
        slot.timeline_id = change.timeline_id;
        slot.timeline_seconds_bits[0] = change.at_seconds.to_bits();
        slot.tempo_bits = change.tempo_bpm.to_bits();
        slot.time_signature_numerator = u32::from(change.time_signature_numerator);
        slot.time_signature_denominator = u32::from(change.time_signature_denominator);
        self.push(slot)
    }

    pub fn send_timeline_events(
        &mut self,
        events: &[TimelineMidiEvent],
    ) -> Result<(), FastIpcError> {
        validate_timeline_events(events)?;
        let mut slot = zeroed_slot();
        slot.kind = KIND_TIMELINE_MIDI;
        slot.timeline_id = events[0].timeline_id;
        slot.message_count = events.len() as u32;
        for (index, event) in events.iter().enumerate() {
            slot.messages[index] = event.message;
            slot.timeline_seconds_bits[index] = event.timeline_seconds.to_bits();
            slot.instance_ids[index] = event.instance_id;
        }
        self.push(slot)
    }
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

fn validate_timeline_events(events: &[TimelineMidiEvent]) -> Result<(), FastIpcError> {
    if events.is_empty() || events.len() > MAX_MIDI_MESSAGES {
        return Err(FastIpcError::TooManyMidiMessages {
            count: events.len(),
            max: MAX_MIDI_MESSAGES,
        });
    }
    let timeline_id = events[0].timeline_id;
    if timeline_id == 0 {
        return Err(FastIpcError::InvalidPayload(
            "timeline id must be non-zero".into(),
        ));
    }
    for event in events {
        if event.timeline_id != timeline_id {
            return Err(FastIpcError::InvalidPayload(
                "one MIDI batch must use one timeline id".into(),
            ));
        }
        validate_instance_id(event.instance_id)?;
        validate_midi_message(event.message)?;
        if !event.timeline_seconds.is_finite() || event.timeline_seconds < 0.0 {
            return Err(FastIpcError::InvalidPayload(
                "timeline seconds must be finite and non-negative".into(),
            ));
        }
    }
    Ok(())
}
