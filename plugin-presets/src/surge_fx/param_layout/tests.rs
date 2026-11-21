use super::*;

#[test]
fn rejects_invalid_versions_arrays_and_duplicate_ids() {
    let original = serde_json::json!({
        "format_version": 2, "generator_version": PARAM_LAYOUT_GENERATOR_VERSION,
        "plugin_version": "8.7.6", "source_tag": "release_xt_8.7.6",
        "source_revision": "1".repeat(40), "sst_effects_revision": "2".repeat(40),
        "layouts": (1..=2).map(|id| serde_json::json!({
            "fx_type": id, "display_name": "Invented",
            "posy_offset": vec![0;12], "active": vec![false;12], "names": vec!["";12],
        })).collect::<Vec<_>>()
    });
    for change in ["version", "array", "duplicate", "empty", "origin"] {
        let mut value = original.clone();
        match change {
            "version" => value["format_version"] = 99.into(),
            "array" => value["layouts"][0]["names"] = serde_json::json!(["short"]),
            "duplicate" => value["layouts"][1]["fx_type"] = value["layouts"][0]["fx_type"].clone(),
            "empty" => value["layouts"] = serde_json::json!([]),
            "origin" => value["source_tag"] = "".into(),
            _ => unreachable!(),
        }
        assert!(
            decode_param_layouts(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{change}"
        );
    }
    assert!(read_param_layouts(Path::new("X:/nonexistent/surge-fx-layout.json")).is_err());
}
