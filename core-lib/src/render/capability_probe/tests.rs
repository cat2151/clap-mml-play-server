//! probe 対象 ID 表と、表への整形の番人。
//!
//! 実プラグインを読む測定そのものは `render/tests.rs` 側（すべて `#[ignore]`）。

use super::*;

/// ADR 0006 の判断は「preset 系の interface が NULL である」ことが根拠なので、
/// 問い合わせ先から preset 系が抜けると根拠ごと消える。
#[test]
fn the_probe_asks_for_both_the_final_and_draft_preset_interfaces() {
    let factories: Vec<&str> = PROBED_FACTORY_IDS
        .iter()
        .map(|id| id.to_str().unwrap())
        .collect();
    let extensions: Vec<&str> = PROBED_EXTENSION_IDS
        .iter()
        .map(|id| id.to_str().unwrap())
        .collect();

    assert!(factories.contains(&"clap.preset-discovery-factory/2"));
    assert!(factories.contains(&"clap.preset-discovery-factory/draft/2"));
    assert!(extensions.contains(&"clap.preset-load/2"));
    assert!(extensions.contains(&"clap.preset-load.draft/2"));
}

/// 同じ ID を 2 回問い合わせても意味が無いので、表は重複無しで持つ。
#[test]
fn the_probed_ids_have_no_duplicates() {
    for ids in [PROBED_FACTORY_IDS, PROBED_EXTENSION_IDS] {
        let mut seen: Vec<&CStr> = Vec::new();
        for id in ids {
            assert!(!seen.contains(id), "重複した probe 対象 ID: {id:?}");
            seen.push(id);
        }
    }
}

#[test]
fn a_missing_interface_list_reads_as_none_instead_of_an_empty_line() {
    assert_eq!(join_or_none(&[], ", "), "(無し)");
    assert_eq!(
        join_or_none(&["clap.state".to_string(), "clap.params".to_string()], ", "),
        "clap.state, clap.params"
    );
}

#[test]
fn dialect_names_spell_out_every_advertised_flag() {
    assert_eq!(dialect_names(NoteDialects::empty()), Vec::<String>::new());
    assert_eq!(dialect_names(NoteDialects::MIDI), vec!["MIDI".to_string()]);
    assert_eq!(
        dialect_names(NoteDialects::CLAP | NoteDialects::MIDI),
        vec!["CLAP".to_string(), "MIDI".to_string()]
    );
}

/// 受け入れ条件に落ちた理由が表から消えると、対応可否の判断がここで止まる。
#[test]
fn the_text_table_keeps_the_rejection_reason() {
    let report = probe_report_for_tests();

    let text = report.to_text();

    assert!(text.contains("受け入れ条件"));
    assert!(text.contains("NG — main output port が 1 ch のプラグインは未対応"));
    assert!(text.contains("voice-info         : なし"));
}

fn probe_report_for_tests() -> PluginProbeReport {
    PluginProbeReport {
        plugin_path: "X.clap".to_string(),
        descriptors: Vec::new(),
        selected: SelectedDescriptor {
            id: "com.example.x".to_string(),
            name: "X".to_string(),
            vendor: "Example".to_string(),
            version: "1.0".to_string(),
        },
        factories: Vec::new(),
        extensions: Vec::new(),
        audio_input_ports: 0,
        audio_output_ports: 1,
        main_output_channels: 1,
        main_output_is_main: true,
        input_note_ports: 1,
        output_note_ports: 0,
        input_note_dialects: vec!["MIDI".to_string()],
        param_count: 0,
        rejected: Some("main output port が 1 ch のプラグインは未対応".to_string()),
    }
}

/// voice-info の値は activate 後でないと読めない。広告しているのに「なし」と書くと
/// ADR 0001 の表が嘘になる。
#[test]
fn an_advertised_voice_info_is_not_reported_as_missing() {
    let mut report = probe_report_for_tests();
    report.extensions = vec!["clap.voice-info".to_string()];

    assert!(report
        .to_text()
        .contains("voice-info         : 広告あり（値は activate 後でないと読めない）"));
}
