use super::*;

const CATHEDRAL_2: &str = r#"<single-fx streaming_version="17">
  <snapshot name="Cathedral 2"
     type="2"
     p0="-8"
     p1="3"
     p2="0.5"
     p3="2.16992"
     p4="0.5"
     p5="-60"
     p6="3"
     p7="0"
     p8="70"
     p9="0.1"
     p10="0"
  />
</single-fx>
"#;

const TWO_SNAPSHOTS: &str = r#"<single-fx streaming_version="24">
    <snapshot type="1" name="A" p0="-3.000000" p0_temposync="1" p1="-2" p1_deactivated="1" p2_extend_range="1" p3="4" p3_deform_type="1" />
    <snapshot type="1" name="B" p0="1" />
</single-fx>
"#;

fn float_report(fx_type: i32, value: f64) -> SurgeFxStateReport {
    SurgeFxStateReport {
        fx_type,
        valtype: [VALTYPE_FLOAT; SURGE_FX_PARAM_COUNT],
        value: [value; SURGE_FX_PARAM_COUNT],
        features: [0; SURGE_FX_PARAM_COUNT],
    }
}

fn attributes(xml: &str) -> std::collections::HashMap<String, String> {
    Element::parse(Cursor::new(xml.as_bytes()))
        .unwrap()
        .attributes
        .into_iter()
        .collect()
}

#[test]
fn parses_every_parameter_of_a_single_snapshot() {
    let snapshots = parse_srgfx(CATHEDRAL_2).unwrap();
    assert_eq!(snapshots.len(), 1);
    let snapshot = &snapshots[0];
    assert_eq!(snapshot.name, "Cathedral 2");
    assert_eq!(snapshot.fx_type, 2);
    assert_eq!(snapshot.streaming_version, 17);
    assert_eq!(snapshot.params[0].value, Some(-8.0));
    assert_eq!(snapshot.params[3].value, Some(2.16992));
    assert_eq!(snapshot.params[10].value, Some(0.0));
    assert_eq!(snapshot.params[11].value, None);
    assert_eq!(snapshot.params[11].raw_value(), 0.0);
}

#[test]
fn parses_flags_and_multiple_snapshots_in_file_order() {
    let snapshots = parse_srgfx(TWO_SNAPSHOTS).unwrap();
    assert_eq!(snapshots.len(), 2);
    let a = &snapshots[0];
    assert_eq!(a.name, "A");
    assert!(a.params[0].temposync);
    assert!(a.params[1].deactivated);
    assert!(a.params[2].extend_range);
    assert_eq!(a.params[2].value, None);
    assert_eq!(a.params[3].deform_type, Some(1));
    assert_eq!(a.params[0].deform_type, None);
    assert_eq!(snapshots[1].name, "B");
    assert_eq!(snapshots[1].streaming_version, 24);
}

/// つなげた 2 文書のうち Surge が読む先頭だけを返す。
#[test]
fn reads_only_the_first_document_of_a_concatenated_file() {
    let concatenated = format!("{CATHEDRAL_2}\n{TWO_SNAPSHOTS}");
    let snapshots = parse_srgfx(&concatenated).unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].name, "Cathedral 2");
}

/// 古い revision の preset は Surge の loader と同じ移行を受ける。
#[test]
fn old_streaming_version_is_migrated_on_parse() {
    let old = TWO_SNAPSHOTS.replace("streaming_version=\"24\"", "streaming_version=\"15\"");
    let snapshots = parse_srgfx(&old).unwrap();
    assert!(!snapshots[0].params[1].deactivated);
    assert_eq!(snapshots[0].streaming_version, 15);
}

#[test]
fn rejects_files_without_snapshot_or_type() {
    assert!(parse_srgfx("<single-fx/>").is_err());
    assert!(parse_srgfx(r#"<single-fx><snapshot name="x"/></single-fx>"#).is_err());
    assert!(parse_srgfx(r#"<other><snapshot name="x" type="1"/></other>"#).is_err());
}

/// Delay は `Channel`（storage 8、posy_offset -15）が GUI の先頭へ来る。
#[test]
fn delay_remap_moves_input_channel_to_the_front() {
    let layout = param_layout(1).unwrap();
    let remap = param_remap(layout);
    assert_eq!(remap, [8, 0, 1, 2, 3, 4, 5, 6, 7, 11, 10, 9]);
}

/// Phaser は GUI の並びが storage の並びと大きく違う。
#[test]
fn phaser_remap_follows_posy_offsets() {
    let layout = param_layout(3).unwrap();
    let remap = param_remap(layout);
    let gui_names: Vec<&str> = remap.iter().map(|index| layout.names[*index]).collect();
    assert_eq!(
        gui_names,
        [
            "Waveform",
            "Rate",
            "Depth",
            "Stereo",
            "Count",
            "Spread",
            "Center",
            "Sharpness",
            "Feedback",
            "Tone",
            "Width",
            "Mix"
        ]
    );
}

#[test]
fn every_layout_remap_is_a_permutation() {
    for layout in SURGE_FX_PARAM_LAYOUTS {
        let mut remap = param_remap(layout);
        remap.sort_unstable();
        assert_eq!(
            remap,
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "fxt={}",
            layout.fx_type
        );
    }
    assert!(param_layout(0).is_none());
}

#[test]
fn state_xml_normalizes_floats_and_keeps_ints_raw() {
    let layout = param_layout(2).unwrap();
    let mut at_zero = float_report(2, 0.0);
    let mut at_one = float_report(2, 0.0);
    // GUI 0 = Pre-Delay (-8..5)、GUI 1 = Room Shape (int 0..3)
    at_zero.value[0] = -8.0;
    at_one.value[0] = 5.0;
    at_zero.valtype[1] = VALTYPE_INT;
    at_one.valtype[1] = VALTYPE_INT;
    at_one.value[1] = 3.0;
    let ranges = SurgeFxParamRanges::from_reports(&at_zero, &at_one).unwrap();

    let snapshot = &parse_srgfx(CATHEDRAL_2).unwrap()[0];
    let xml = snapshot_state_xml(snapshot, layout, &ranges).unwrap();
    let attributes = attributes(&xml);
    assert_eq!(attributes["fxt"], "2");
    assert_eq!(attributes["streamingVersion"], "2");
    assert_eq!(attributes["fxp_0"], "0");
    assert_eq!(attributes["surgevaltype_1"], "0");
    assert_eq!(attributes["surgeval_1"], "3");
    assert_eq!(attributes["fxp_param_features_0"], "0");
    // GUI 2 = Size (storage 2) は range が 0 幅なので 0 に落ちる。
    assert_eq!(attributes["fxp_2"], "0");
}

#[test]
fn state_xml_encodes_feature_bits() {
    let layout = param_layout(1).unwrap();
    let ranges =
        SurgeFxParamRanges::from_reports(&float_report(1, 0.0), &float_report(1, 1.0)).unwrap();
    let snapshot = &parse_srgfx(TWO_SNAPSHOTS).unwrap()[0];
    let xml = snapshot_state_xml(snapshot, layout, &ranges).unwrap();
    let attributes = attributes(&xml);
    // Delay の GUI 1 = storage 0（temposync）、GUI 2 = storage 1（deactivated）、
    // GUI 3 = storage 2（extend）
    assert_eq!(attributes["fxp_param_features_1"], "1");
    assert_eq!(attributes["fxp_param_features_2"], "8");
    assert_eq!(attributes["fxp_param_features_3"], "2");
    assert_eq!(attributes["fxp_1"], "-3");
}

#[test]
fn rejects_mismatched_fx_type() {
    let layout = param_layout(1).unwrap();
    let ranges =
        SurgeFxParamRanges::from_reports(&float_report(1, 0.0), &float_report(1, 0.0)).unwrap();
    let snapshot = &parse_srgfx(CATHEDRAL_2).unwrap()[0];
    assert!(snapshot_state_xml(snapshot, layout, &ranges).is_err());
    assert!(
        SurgeFxParamRanges::from_reports(&float_report(1, 0.0), &float_report(2, 1.0)).is_err()
    );
}

#[test]
fn calibration_and_report_round_trip_through_the_xml() {
    let xml = calibration_state_xml(11, 1.0);
    assert_eq!(attributes(&xml)["fxp_11"], "1");
    let report = parse_state_xml(
        r#"<surgefx streamingVersion="2" fxt="11" surgevaltype_0="2" surgeval_0="-4.5" fxp_param_features_0="9" surgevaltype_1="0" surgeval_1="2"/>"#,
    )
    .unwrap();
    assert_eq!(report.fx_type, 11);
    assert_eq!(report.value[0], -4.5);
    assert_eq!(report.features[0], 9);
    assert_eq!(report.valtype[1], VALTYPE_INT);
    assert_eq!(report.value[1], 2.0);
    assert_eq!(report.valtype[2], VALTYPE_FLOAT);
}

#[test]
fn juce_xml_binary_round_trips() {
    let xml = r#"<?xml version="1.0"?><surgefx fxt="1"/>"#;
    let bytes = juce_xml::encode(xml);
    assert_eq!(&bytes[..4], b"VC2!");
    assert_eq!(bytes.len(), xml.len() + 9);
    assert_eq!(*bytes.last().unwrap(), 0);
    assert_eq!(juce_xml::decode(&bytes).unwrap(), xml);
    assert!(juce_xml::decode(b"nope").is_err());
}
