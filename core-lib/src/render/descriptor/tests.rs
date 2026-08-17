use super::*;

fn descriptor(id: &str) -> SelectedDescriptor {
    SelectedDescriptor {
        id: id.to_string(),
        name: "Test Plugin".to_string(),
        vendor: "Test Vendor".to_string(),
        version: "1.0.0".to_string(),
    }
}

fn capabilities(inputs: u32, outputs: u32, dialects: NoteDialects) -> PluginCapabilities {
    PluginCapabilities {
        audio_input_ports: inputs,
        audio_output_ports: outputs,
        main_output_channels: 2,
        main_output_is_main: true,
        input_note_ports: 1,
        input_note_dialects: dialects,
    }
}

#[test]
fn no_descriptor_is_an_error() {
    let error = choose_descriptor(Vec::new()).unwrap_err();

    assert!(error.to_string().contains("見つからない"));
}

#[test]
fn a_single_descriptor_is_selected() {
    let selected = choose_descriptor(vec![descriptor("com.digital-suburban.dexed")]).unwrap();

    assert_eq!(selected.id, "com.digital-suburban.dexed");
    assert_eq!(selected.version, "1.0.0");
}

#[test]
fn a_descriptor_without_an_id_is_an_error() {
    let error = choose_descriptor(vec![descriptor("")]).unwrap_err();

    assert!(error.to_string().contains("プラグインID"));
}

#[test]
fn multiple_descriptors_report_every_id() {
    let error = choose_descriptor(vec![
        descriptor("org.surge-synth-team.surge-xt"),
        descriptor("com.digital-suburban.dexed"),
    ])
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("org.surge-synth-team.surge-xt"));
    assert!(message.contains("com.digital-suburban.dexed"));
}

/// Surge XT は output port を 3 本、Dexed は 1 本広告する（実測）。どちらも port 0 の
/// main だけを使うので、本数そのものは受け入れ条件にしない。
#[test]
fn surge_and_dexed_audio_topologies_are_both_accepted() {
    let surge = capabilities(1, 3, NoteDialects::CLAP | NoteDialects::MIDI);
    let dexed = capabilities(0, 1, NoteDialects::MIDI);

    assert!(validate_capabilities(&surge, &descriptor("surge")).is_ok());
    assert!(validate_capabilities(&dexed, &descriptor("dexed")).is_ok());
}

#[test]
fn a_plugin_without_an_audio_output_port_is_rejected() {
    let error = validate_capabilities(
        &capabilities(0, 0, NoteDialects::CLAP),
        &descriptor("no-out"),
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("audio output port"));
    assert!(message.contains("no-out"));
}

#[test]
fn a_first_output_port_that_is_not_the_main_port_is_rejected() {
    let mut aux_first = capabilities(0, 2, NoteDialects::CLAP);
    aux_first.main_output_is_main = false;

    let error = validate_capabilities(&aux_first, &descriptor("aux-first")).unwrap_err();

    assert!(error.to_string().contains("main ではない"));
}

#[test]
fn a_non_stereo_main_output_is_rejected() {
    let mut mono_out = capabilities(0, 1, NoteDialects::CLAP);
    mono_out.main_output_channels = 1;

    let error = validate_capabilities(&mono_out, &descriptor("mono-out")).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("main output port"));
    assert!(message.contains("mono-out"));
}

#[test]
fn a_plugin_without_a_note_input_port_is_rejected() {
    let mut without_note_port = capabilities(0, 1, NoteDialects::CLAP);
    without_note_port.input_note_ports = 0;

    let error = validate_capabilities(&without_note_port, &descriptor("effect")).unwrap_err();

    assert!(error.to_string().contains("note input port"));
}

#[test]
fn clap_wins_when_both_dialects_are_advertised() {
    let dialect = resolve_note_dialect(
        &capabilities(1, 1, NoteDialects::CLAP | NoteDialects::MIDI),
        &descriptor("surge"),
    )
    .unwrap();

    assert_eq!(dialect, NoteEventDialect::Clap);
}

#[test]
fn midi_only_falls_back_to_midi_events() {
    let dialect = resolve_note_dialect(
        &capabilities(0, 1, NoteDialects::MIDI),
        &descriptor("dexed"),
    )
    .unwrap();

    assert_eq!(dialect, NoteEventDialect::Midi);
}

#[test]
fn neither_clap_nor_midi_is_an_error() {
    let error = resolve_note_dialect(
        &capabilities(0, 1, NoteDialects::MIDI2),
        &descriptor("midi2-only"),
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("dialect"));
    assert!(message.contains("midi2-only"));
}
