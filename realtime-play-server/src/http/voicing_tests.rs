use super::*;
use cmrt_core::voicing::ProbeReport;
use cmrt_core::{PatchVoicing, VoicingReport};
use std::sync::Mutex;

#[derive(Default)]
struct ProbePlayer {
    patches: Mutex<Vec<Option<String>>>,
}

impl PlayerHandle for ProbePlayer {
    fn play_smf(&self, _smf: Vec<u8>) -> Result<()> {
        Ok(())
    }

    fn play_mml(&self, _mml: String) -> Result<()> {
        Ok(())
    }

    fn send_midi(
        &self,
        _messages: Vec<[u8; 3]>,
        _offsets: Vec<u32>,
        _patch: Option<String>,
    ) -> Result<()> {
        Ok(())
    }

    fn prepare_live_patch(&self, _patch: Option<String>) -> Result<()> {
        Ok(())
    }

    fn prepare_live_patch_with_voicing(&self, patch: Option<String>) -> Result<VoicingReport> {
        self.patches.lock().unwrap().push(patch);
        Ok(VoicingReport {
            decision: PatchVoicing::Mono,
            probe: ProbeReport {
                result: PatchVoicing::Mono,
                ended_note_ids: vec![1],
                blocks: 1,
            },
            voice_info: None,
            surge: None,
            disagreement: false,
        })
    }

    fn set_live_buffer_multiplier(&self, _multiplier: u8) -> Result<()> {
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        Ok(())
    }
}

#[test]
fn live_patch_probe_returns_json_report_after_preparing_patch() {
    let player = ProbePlayer::default();
    let request = HttpRequest {
        method: "POST".into(),
        path: "/live-patch-probe".into(),
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("content-length".into(), "26".into()),
        ],
        body: br#"{"patch":"Leads/Mono.fxp"}"#.to_vec(),
    };
    let mut response = Vec::new();

    handle_live_patch_probe_request(&mut response, &player, request).unwrap();

    let response = String::from_utf8(response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "response was {response:?}"
    );
    assert!(response.contains("Content-Type: application/json\r\n"));
    assert!(response.contains(r#""decision":"mono""#));
    assert_eq!(
        *player.patches.lock().unwrap(),
        vec![Some("Leads/Mono.fxp".into())]
    );
}
