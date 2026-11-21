use super::*;

#[test]
fn apricot_factory_and_user_references_resolve_to_the_same_kind() {
    let mut apricot = fake_kind("Apricot", PatchForm::ApricotPreset, None);
    apricot.core_cfg.patch_base = PatchBase::per_root(&[
        "X:/plugins/Apricot.clap".into(),
        "X:/Documents/Apricot Synthesizer/Patches".into(),
    ]);
    let kinds = vec![fake_kind("Surge XT", PatchForm::StateFile, None), apricot];
    let bases = PatchBases::from_kinds(&kinds);
    for (wire, expected) in [
        (
            "Apricot.clap/patches/Factory Pads/Pad.synthpatch:",
            "X:/plugins/Apricot.clap/patches/Factory Pads/Pad.synthpatch",
        ),
        (
            "Patches/User/Pad.synthpatch:",
            "X:/Documents/Apricot Synthesizer/Patches/User/Pad.synthpatch",
        ),
    ] {
        assert_eq!(kind_for_patch(&kinds, 0, Some(wire)), Ok(1));
        let resolved = bases.resolve(wire).unwrap();
        assert_eq!(
            Path::new(parse_patch_wire(&resolved).unwrap().path),
            Path::new(expected)
        );
    }
    assert!(kind_for_patch(&kinds[..1], 0, Some("Patches/Pad.synthpatch:")).is_err());
}
