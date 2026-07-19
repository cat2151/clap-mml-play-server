use super::*;
use crate::voicing::{
    build_voicing_report, read_plugin_diagnostics, PatchVoicing, ProbeReport, VoicingReport,
};
use std::sync::atomic::{AtomicU64, Ordering};

const PROBE_BLOCKS: u32 = 4;
const PATCH_SETTLE_BLOCKS: u32 = 1;
const PROBE_KEYS: [u16; 2] = [60, 64];
const PROBE_NOTE_ID_PAIR_COUNT: u64 = (i32::MAX as u64 - 1) / 2;
// NOTE_END can arrive after cleanup, so IDs must not be reused by the next probe.
static NEXT_PROBE_NOTE_ID_PAIR: AtomicU64 = AtomicU64::new(0);

fn next_probe_note_ids() -> [u32; 2] {
    let pair = NEXT_PROBE_NOTE_ID_PAIR.fetch_add(1, Ordering::Relaxed) % PROBE_NOTE_ID_PAIR_COUNT;
    let first = (pair * 2 + 1) as u32;
    [first, first + 1]
}

pub(super) fn first_plugin_id(entry: &PluginEntry) -> Result<String> {
    let plugin_factory = entry
        .get_plugin_factory()
        .ok_or_else(|| anyhow::anyhow!("PluginFactory が見つからない"))?;
    let plugin_descriptor = plugin_factory
        .plugin_descriptors()
        .next()
        .ok_or_else(|| anyhow::anyhow!("プラグインディスクリプタが見つからない"))?;
    Ok(plugin_descriptor
        .id()
        .ok_or_else(|| anyhow::anyhow!("プラグインIDが見つからない"))?
        .to_string_lossy()
        .into_owned())
}

impl RealtimeRenderer {
    /// 現在のpatchへ2音のCLAP NoteOnを送り、NOTE_ENDによるvoice数判定を行う。
    /// 生成された音声は返さず、probe終了後にprocessorをresetする。
    pub fn probe_voicing(&mut self) -> Result<VoicingReport> {
        self.settle_patch_state()?;
        let (voice_info, surge) = read_plugin_diagnostics(
            self.plugin_instance
                .as_mut()
                .expect("plugin instance is always present while renderer is alive"),
            &self.plugin_id,
        );
        let result = self.run_voicing_probe();
        self.reset();
        result.map(|probe| build_voicing_report(probe, voice_info, surge))
    }

    fn run_voicing_probe(&mut self) -> Result<ProbeReport> {
        let probe_note_ids = next_probe_note_ids();
        let mut note_ons = EventBuffer::new();
        for (key, note_id) in PROBE_KEYS.into_iter().zip(probe_note_ids) {
            note_ons.push(&NoteOnEvent::new(
                0,
                Pckn::new(0u16, 0u16, key, note_id),
                100.0 / 127.0,
            ));
        }

        let empty = EventBuffer::new();
        let mut ended_note_ids = Vec::new();
        let mut blocks = 0;
        for block in 0..PROBE_BLOCKS {
            let input = if block == 0 { &note_ons } else { &empty };
            let processed = self.process_chunk_with_events(self.buf_size as u32, input)?;
            blocks += 1;
            ended_note_ids.extend(
                processed
                    .ended_note_ids
                    .into_iter()
                    .filter(|id| probe_note_ids.contains(id)),
            );
            if !ended_note_ids.is_empty() {
                break;
            }
        }

        let mut note_offs = EventBuffer::new();
        for (key, note_id) in PROBE_KEYS.into_iter().zip(probe_note_ids) {
            note_offs.push(&NoteOffEvent::new(
                0,
                Pckn::new(0u16, 0u16, key, note_id),
                0.0,
            ));
        }
        let _ = self.process_chunk_with_events(self.buf_size as u32, &note_offs)?;

        ended_note_ids.sort_unstable();
        ended_note_ids.dedup();
        Ok(ProbeReport {
            result: if ended_note_ids.is_empty() {
                PatchVoicing::Poly
            } else {
                PatchVoicing::Mono
            },
            ended_note_ids,
            blocks,
        })
    }

    fn settle_patch_state(&mut self) -> Result<()> {
        let empty = EventBuffer::new();
        // Let state-load work and late NOTE_END events finish before reading params or probing.
        for _ in 0..PATCH_SETTLE_BLOCKS {
            let _ = self.process_chunk_with_events(self.buf_size as u32, &empty)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_probes_use_disjoint_note_ids() {
        let first = next_probe_note_ids();
        let second = next_probe_note_ids();

        assert!(first.into_iter().all(|id| !second.contains(&id)));
        assert_eq!(first[1], first[0] + 1);
        assert_eq!(second[1], second[0] + 1);
    }
}
