use super::*;

mod apricot;

fn fake_kind(name: &str, patch_form: PatchForm, patches_dir: Option<&str>) -> PluginKind {
    PluginKind {
        key: PluginKey::from_identity(None, &format!("{name}.clap")),
        name: name.to_string(),
        plugin_path: format!("{name}.clap"),
        patch_form,
        patch_tag: "",
        core_cfg: CoreConfig {
            patch_base: crate::PatchBase::from(patches_dir.map(str::to_string)),
            ..Default::default()
        },
    }
}

fn base_of<'a>(bases: &'a PatchBases, wire: &str) -> &'a crate::PatchBase {
    bases.base_for(&parse_patch_wire(wire).unwrap())
}

#[test]
fn obxf_and_surge_route_to_their_own_instances_and_roots() {
    let mut obxf = fake_kind("OB-Xf", PatchForm::StateFile, Some("X:/obxf"));
    obxf.patch_tag = cmrt_server_config::OBXF_PATCH_TAG;
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, Some("X:/surge")),
        obxf,
    ];
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Basses/Bass.fxp:")).unwrap(),
        0
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Basses/Bass.fxp:o")).unwrap(),
        1
    );
    let bases = PatchBases::from_kinds(&kinds);
    assert_eq!(
        base_of(&bases, "Basses/Bass.fxp:o"),
        &PatchBase::Shared("X:/obxf".into())
    );
}

#[test]
fn duplicate_patch_forms_are_ambiguous_instead_of_using_the_first_plugin() {
    let kinds = vec![
        fake_kind("Surge XT A", PatchForm::StateFile, None),
        fake_kind("Surge XT B", PatchForm::StateFile, None),
    ];

    let error = kind_for_patch(&kinds, 0, Some("Keys/Piano.fxp:")).unwrap_err();

    assert!(error.contains("一意に決められない"), "{error}");
    assert!(error.contains("Surge XT A"), "{error}");
    assert!(error.contains("Surge XT B"), "{error}");
}

/// Surge のインスタンスへ DX7 の SysEx を送っても、Surge は理解できない 163 byte を
/// **黙って無視する**。エラーにならないので、引き当てられない時点で落とす。
#[test]
fn a_patch_without_a_matching_plugin_is_an_error_instead_of_a_silent_no_op() {
    let kinds = vec![fake_kind("Surge XT", PatchForm::StateFile, None)];

    let error = kind_for_patch(&kinds, 0, Some("Dexed_01.syx/00 Say Again.:")).unwrap_err();

    assert!(error.contains("Dexed_01.syx"), "{error}");
    assert!(error.contains("Surge XT"), "{error}");
}

/// 無指定の音色が鳴るプラグインは常に既定 1 つ。これが崩れると、MML 文字列を鍵にしている
/// cache が「無指定の行」で衝突する。
#[test]
fn an_unspecified_patch_always_resolves_to_the_default_plugin() {
    let kinds = vec![
        fake_kind("Dexed", PatchForm::Cartridge, None),
        fake_kind("Surge XT", PatchForm::StateFile, None),
    ];

    assert_eq!(kind_for_patch(&kinds, 0, None).unwrap(), 0);
    assert_eq!(kind_for_patch(&kinds, 1, None).unwrap(), 1);
}

#[test]
fn the_patch_form_decides_which_plugin_a_patch_needs() {
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, None),
        fake_kind("Dexed", PatchForm::Cartridge, None),
    ];

    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Keys/Piano.fxp:")).unwrap(),
        0
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Dexed_01.syx/00 Say Again.:")).unwrap(),
        1
    );
}

/// 音色置き場はプラグインごとに別の場所なので、基点も形ごとに分かれていないと
/// cartridge の相対パスが Surge の音色置き場へ join されてしまう。
#[test]
fn patch_bases_keep_one_root_per_patch_form() {
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, Some("/surge")),
        fake_kind("Dexed", PatchForm::Cartridge, Some("/dexed")),
    ];

    let bases = PatchBases::from_kinds(&kinds);

    assert_eq!(
        base_of(&bases, "Keys/Piano.fxp:").scan_dir(),
        Some("/surge")
    );
    assert_eq!(
        base_of(&bases, "Dexed_01.syx/00 Say.:").scan_dir(),
        Some("/dexed")
    );
}

/// Vaporizer2 を足した瞬間、`StateFile` が「Surge の `.fxp`」と同義でなくなる。
/// 3 種別が同居しても、それぞれの形が自分のプラグインへ行くこと。
#[test]
fn three_plugins_each_take_only_their_own_patch_form() {
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, None),
        fake_kind("Dexed", PatchForm::Cartridge, None),
        fake_kind("Vaporizer2", PatchForm::Vvp, None),
    ];

    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Keys/Piano.fxp:")).unwrap(),
        0
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Dexed_01.syx/00 Say Again.:")).unwrap(),
        1
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("AR Accent Arp.vvp:")).unwrap(),
        2
    );
}

/// `.vvp` が `StateFile` に混ざっていると、Surge のインスタンスへ Vaporizer2 の
/// state が流れる。Surge が載っているだけの環境では**引き当てられずにエラー**が正しい。
#[test]
fn a_vvp_patch_is_not_mistaken_for_a_surge_state_file() {
    let kinds = vec![fake_kind("Surge XT", PatchForm::StateFile, None)];

    let error = kind_for_patch(&kinds, 0, Some("PD Emily.vvp:")).unwrap_err();

    assert!(error.contains("PD Emily.vvp"), "{error}");
    assert!(error.contains("Surge XT"), "{error}");
}

/// 既定プラグインが Vaporizer2 でも、無指定は既定へ行く（形の一致より既定が先）。
#[test]
fn vaporizer2_can_be_the_default_plugin() {
    let kinds = vec![
        fake_kind("Vaporizer2", PatchForm::Vvp, None),
        fake_kind("Surge XT", PatchForm::StateFile, None),
    ];

    assert_eq!(kind_for_patch(&kinds, 0, None).unwrap(), 0);
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Keys/Piano.fxp:")).unwrap(),
        1
    );
}

#[test]
fn patch_bases_keep_a_separate_root_for_vvp() {
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, Some("/surge")),
        fake_kind("Dexed", PatchForm::Cartridge, Some("/dexed")),
        fake_kind("Vaporizer2", PatchForm::Vvp, Some("/vaporizer2")),
    ];

    let bases = PatchBases::from_kinds(&kinds);

    assert_eq!(
        base_of(&bases, "Keys/Piano.fxp:").scan_dir(),
        Some("/surge")
    );
    assert_eq!(
        base_of(&bases, "Dexed_01.syx/00 Say.:").scan_dir(),
        Some("/dexed")
    );
    assert_eq!(
        base_of(&bases, "AR Accent Arp.vvp:").scan_dir(),
        Some("/vaporizer2")
    );
}

#[test]
fn four_plugins_route_each_patch_to_its_own_instance() {
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, None),
        fake_kind("Dexed", PatchForm::Cartridge, None),
        fake_kind("Vaporizer2", PatchForm::Vvp, None),
        fake_kind("Floe", PatchForm::FloePreset, None),
    ];

    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Keys/Piano.fxp:")).unwrap(),
        0
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Dexed.syx/00 Init:")).unwrap(),
        1
    );
    assert_eq!(kind_for_patch(&kinds, 0, Some("PD Emily.vvp:")).unwrap(), 2);
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Harp/Realistic.floe-preset:")).unwrap(),
        3
    );
}

#[test]
fn five_plugins_route_sfz_only_to_sforzando() {
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, None),
        fake_kind("Dexed", PatchForm::Cartridge, None),
        fake_kind("Vaporizer2", PatchForm::Vvp, None),
        fake_kind("Floe", PatchForm::FloePreset, None),
        fake_kind("Sforzando", PatchForm::Sfz, None),
    ];

    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Garritan/Glockenspiel.sfz:")).unwrap(),
        4
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Garritan/Glockenspiel.SFZ:")).unwrap(),
        4
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("TableWarp2/Presets/Keys/Airy Bells.ariax:")).unwrap(),
        4
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Keys\\Airy Bells.ARIAX:")).unwrap(),
        4
    );
}

#[test]
fn an_sfz_patch_without_sforzando_is_an_error() {
    let kinds = vec![fake_kind("Surge XT", PatchForm::StateFile, None)];

    let error = kind_for_patch(&kinds, 0, Some("Garritan/Glockenspiel.sfz:")).unwrap_err();

    assert!(error.contains("Glockenspiel.sfz"), "{error}");
    assert!(error.contains("Surge XT"), "{error}");
}

#[test]
fn patch_bases_keep_a_separate_root_for_floe() {
    let bases = PatchBases::from_all_bases(
        Some("/surge"),
        Some("/dexed"),
        Some("/vaporizer2"),
        Some("/floe"),
        None,
    );

    assert_eq!(
        base_of(&bases, "Keys/Piano.fxp:").scan_dir(),
        Some("/surge")
    );
    assert_eq!(
        base_of(&bases, "Dexed.syx/00 Init:").scan_dir(),
        Some("/dexed")
    );
    assert_eq!(
        base_of(&bases, "PD Emily.vvp:").scan_dir(),
        Some("/vaporizer2")
    );
    assert_eq!(
        base_of(&bases, "Harp/Realistic.floe-preset:").scan_dir(),
        Some("/floe")
    );
}

#[test]
fn patch_bases_keep_a_separate_root_for_sfz() {
    let bases = PatchBases::from_all_bases(
        Some("/surge"),
        Some("/dexed"),
        Some("/vaporizer2"),
        Some("/floe"),
        Some("/sfz"),
    );

    assert_eq!(
        base_of(&bases, "Keys/Piano.fxp:").scan_dir(),
        Some("/surge")
    );
    assert_eq!(
        base_of(&bases, "Dexed.syx/00 Init:").scan_dir(),
        Some("/dexed")
    );
    assert_eq!(
        base_of(&bases, "PD Emily.vvp:").scan_dir(),
        Some("/vaporizer2")
    );
    assert_eq!(
        base_of(&bases, "Harp/Realistic.floe-preset:").scan_dir(),
        Some("/floe")
    );
    assert_eq!(
        base_of(&bases, "Garritan/Glockenspiel.sfz:").scan_dir(),
        Some("/sfz")
    );
}

#[test]
fn legacy_patch_bases_constructor_leaves_floe_unconfigured() {
    let bases = PatchBases::from_bases(Some("/surge"), Some("/dexed"), Some("/vaporizer2"));

    assert_eq!(
        base_of(&bases, "Harp/Realistic.floe-preset:").scan_dir(),
        None
    );
}

/// gate（かつての `CMRT_CACHE_PLAYER=1`）を撤去したことの回帰テスト。
///
/// 環境変数を一切触らずに cache-player が種別一覧へ載ること。ここが崩れると
/// 「サーバー起動時のフラグを忘れたせいで、DAW の操作はすべて成功したまま無音」
/// という、エラーも警告も出ない失敗が復活する。
#[test]
fn the_cache_player_is_always_in_the_kind_list_without_any_env_var() {
    let cfg = ServerConfig::from_toml_str(
        r#"
output_midi = "output.mid"
output_wav = "output.wav"
sample_rate = 48000
buffer_size = 512
"#,
    )
    .unwrap();

    let kinds = plugin_kinds(&cfg, &CoreConfig::default());

    let cache_player = kinds
        .iter()
        .find(|kind| kind.patch_form == PatchForm::CacheWav)
        .expect("cache-player が種別一覧に無い");
    assert_eq!(cache_player.name, CACHE_PLAYER_PROFILE_NAME);
    assert_eq!(
        cache_player.core_cfg.plugin_id.as_deref(),
        Some(CACHE_PLAYER_PLUGIN_ID)
    );
    // 組み込みなので `.clap` ファイルではなく擬似パスで指す。
    assert_eq!(
        cache_player.plugin_path,
        crate::builtin_plugin_path(CACHE_PLAYER_PLUGIN_ID)
    );
    // 音源は DAW が絶対パスで指すので基点を持たない。
    assert_eq!(cache_player.core_cfg.patch_base, crate::PatchBase::None);
    // 起動時の音色は既定プラグイン向けの指定なので持ち込まない。
    assert!(cache_player.core_cfg.patch_path.is_none());
}

#[test]
fn an_untagged_patch_is_a_legacy_error_instead_of_a_routing_guess() {
    let kinds = vec![fake_kind("Surge XT", PatchForm::StateFile, None)];

    let error = kind_for_patch(&kinds, 0, Some("Keys/Piano.fxp")).unwrap_err();

    assert!(error.contains("旧形式"), "{error}");
}

#[test]
fn resolving_a_wire_joins_the_owner_base_and_keeps_the_tag() {
    let bases = PatchBases::from_bases(Some("/surge"), None, None);

    let resolved = bases.resolve("Keys/Piano.fxp:").unwrap();

    assert!(resolved.ends_with(":"), "{resolved}");
    let wire = parse_patch_wire(&resolved).unwrap();
    assert_eq!(
        std::path::Path::new(wire.path),
        std::path::Path::new("/surge").join("Keys/Piano.fxp")
    );
    assert!(matches!(
        bases.resolve("Keys/Piano.fxp"),
        Err(PatchWireError::Legacy { .. })
    ));
}

/// `.h2p` は同じ形でもタグで所有者と基点が分かれる。
#[test]
fn the_tag_splits_h2p_between_tyrelln6_and_zebralette3() {
    let mut zebralette3 = fake_kind("Zebralette3", PatchForm::H2p, Some("/z3"));
    zebralette3.patch_tag = "z";
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, Some("/surge")),
        fake_kind("TyrellN6", PatchForm::H2p, Some("/n6")),
        zebralette3,
    ];

    assert_eq!(
        kind_for_patch(&kinds, 0, Some("1 Poly Keys/x.h2p:z")),
        Ok(2)
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("01 Basses/Abgrund.h2p:")),
        Ok(1)
    );

    let bases = PatchBases::from_kinds(&kinds);
    let resolved = bases.resolve("1 Poly Keys/x.h2p:z").unwrap();
    let wire = parse_patch_wire(&resolved).unwrap();
    assert_eq!(wire.tag, "z");
    assert_eq!(
        std::path::Path::new(wire.path),
        std::path::Path::new("/z3").join("1 Poly Keys/x.h2p")
    );
    let resolved = bases.resolve("01 Basses/Abgrund.h2p:").unwrap();
    assert_eq!(
        std::path::Path::new(parse_patch_wire(&resolved).unwrap().path),
        std::path::Path::new("/n6").join("01 Basses/Abgrund.h2p")
    );

    let without_z3 = &kinds[..2];
    let error = kind_for_patch(without_z3, 0, Some("1 Poly Keys/x.h2p:z")).unwrap_err();
    assert!(error.contains("インストールされていない"), "{error}");
}
