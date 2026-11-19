//! MML・config・ランダム選択・履歴の各入口が、タグの無い patch 文字列と旧キーを拒むこと。

use super::*;
use crate::pipeline::history::history_line;
use crate::pipeline::patch_resolution::resolve_effective_patch;
use crate::{PatchBase, PatchBases, PluginKey, PluginKind};
use cmrt_server_config::PatchForm;

fn config_with_base(base: &std::path::Path) -> CoreConfig {
    CoreConfig {
        sample_rate: 48_000.0,
        buffer_size: 512,
        patch_base: PatchBase::Shared(base.to_string_lossy().into_owned()),
        ..Default::default()
    }
}

#[test]
fn the_old_patch_key_is_an_error_instead_of_the_default_patch() {
    for mml in [
        r#"{"Surge XT patch": "x.fxp:"} c"#,
        r#"{"Surge XT patch filter": "Pads"} c"#,
    ] {
        let error =
            prepare_realtime_play(mml, &CoreConfig::default(), &PatchBases::default()).unwrap_err();
        assert!(format!("{error:#}").contains("旧キー"), "{mml}: {error:#}");
        let error = embedded_patch_ref(mml).unwrap_err();
        assert!(format!("{error:#}").contains("旧キー"), "{mml}: {error:#}");
    }
}

#[test]
fn an_untagged_patch_in_the_mml_is_a_legacy_error() {
    let error = prepare_realtime_play(
        r#"{"patch": "x.fxp"} c"#,
        &CoreConfig::default(),
        &PatchBases::default(),
    )
    .unwrap_err();

    assert!(format!("{error:#}").contains("旧形式"), "{error:#}");
}

#[test]
fn a_tagged_patch_in_the_mml_is_resolved_and_keeps_its_tag() {
    let base = std::env::temp_dir().join("cmrt_patch_entries_base");
    let prepared = prepare_realtime_play(
        r#"{"patch": "Pads/Pad 1.fxp:"} c"#,
        &CoreConfig::default(),
        &PatchBases::from_bases(Some(&base.to_string_lossy()), None, None),
    )
    .unwrap();

    let patch = prepared.patch_path.unwrap();
    let wire = crate::parse_patch_wire(&patch).unwrap();
    assert_eq!(wire.tag, "");
    assert_eq!(
        std::path::Path::new(wire.path),
        base.join("Pads").join("Pad 1.fxp")
    );
    assert_eq!(
        embedded_patch_ref(r#"{"patch": "Pads/Pad 1.fxp:"} c"#)
            .unwrap()
            .as_deref(),
        Some("Pads/Pad 1.fxp:")
    );
}

#[test]
fn unknown_keys_are_kept_and_a_missing_patch_key_is_none() {
    assert_eq!(embedded_patch_ref(r#"{"beat": "4/4"} c"#).unwrap(), None);
    assert_eq!(embedded_patch_ref("cde").unwrap(), None);
}

#[test]
fn a_random_patch_is_returned_as_a_default_owner_wire() {
    let base = std::env::temp_dir().join("cmrt_patch_entries_random");
    std::fs::create_dir_all(base.join("Pads")).unwrap();
    std::fs::write(base.join("Pads").join("Only.fxp"), b"x").unwrap();

    let patch = resolve_effective_patch(None, &config_with_base(&base), true)
        .unwrap()
        .unwrap();

    let wire = crate::parse_patch_wire(&patch).unwrap();
    assert_eq!(wire.tag, "");
    assert!(wire.path.ends_with("Only.fxp"), "{patch}");
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn the_history_line_writes_a_relative_wire_under_the_new_key() {
    let base = std::env::temp_dir().join("cmrt_patch_entries_history");
    let absolute = format!("{}:", base.join("Pads").join("Pad 1.fxp").display());

    let line = history_line(
        r#"{"patch": "Pads/Pad 1.fxp:"} cde"#,
        &Some(absolute),
        &config_with_base(&base),
    )
    .unwrap();

    assert_eq!(line, "{\"patch\": \"Pads/Pad 1.fxp:\"} cde\n");
    let error = history_line(
        "cde",
        &Some("Pads/Pad 1.fxp".into()),
        &config_with_base(&base),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("旧形式"), "{error:#}");
}

fn kind(name: &str, form: PatchForm, tag: &'static str, base: &std::path::Path) -> PluginKind {
    PluginKind {
        key: PluginKey::from_identity(None, &format!("{name}.clap")),
        name: name.to_string(),
        plugin_path: format!("{name}.clap"),
        patch_form: form,
        patch_tag: tag,
        core_cfg: config_with_base(base),
    }
}

/// realtime の `/play-mml` は既定プラグイン（先頭）の基点でなく、音色の所有者の基点で解決する。
#[test]
fn a_realtime_mml_patch_is_resolved_against_its_owners_base() {
    let root = std::env::temp_dir().join("cmrt_patch_entries_owner");
    let (surge, n6, z3) = (root.join("surge"), root.join("n6"), root.join("z3"));
    let kinds = vec![
        kind("Surge XT", PatchForm::StateFile, "", &surge),
        kind("TyrellN6", PatchForm::H2p, "", &n6),
        kind("Zebralette3", PatchForm::H2p, "z", &z3),
    ];
    let bases = PatchBases::from_kinds(&kinds);

    for (mml, base, rel, tag) in [
        (
            r#"{"patch": "1 Poly Keys/x.h2p:z"} c"#,
            &z3,
            "1 Poly Keys/x.h2p",
            "z",
        ),
        (
            r#"{"patch": "01 Basses/y.h2p:"} c"#,
            &n6,
            "01 Basses/y.h2p",
            "",
        ),
        (
            r#"{"patch": "Pads/Pad 1.fxp:"} c"#,
            &surge,
            "Pads/Pad 1.fxp",
            "",
        ),
    ] {
        let patch = prepare_realtime_play(mml, &kinds[0].core_cfg, &bases)
            .unwrap()
            .patch_path
            .unwrap();
        let wire = crate::parse_patch_wire(&patch).unwrap();
        assert_eq!(wire.tag, tag, "{mml}");
        assert_eq!(std::path::Path::new(wire.path), base.join(rel), "{mml}");
    }
}
