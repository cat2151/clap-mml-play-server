use super::*;

fn fake_kind(name: &str, patch_form: PatchForm, patches_dir: Option<&str>) -> PluginKind {
    PluginKind {
        name: name.to_string(),
        plugin_path: format!("{name}.clap"),
        patch_form,
        core_cfg: CoreConfig {
            patches_dir: patches_dir.map(str::to_string),
            ..Default::default()
        },
    }
}

/// Surge のインスタンスへ DX7 の SysEx を送っても、Surge は理解できない 163 byte を
/// **黙って無視する**。エラーにならないので、引き当てられない時点で落とす。
#[test]
fn a_patch_without_a_matching_plugin_is_an_error_instead_of_a_silent_no_op() {
    let kinds = vec![fake_kind("Surge XT", PatchForm::StateFile, None)];

    let error = kind_for_patch(&kinds, 0, Some("Dexed_01.syx/00 Say Again.")).unwrap_err();

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
        kind_for_patch(&kinds, 0, Some("Keys/Piano.fxp")).unwrap(),
        0
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Dexed_01.syx/00 Say Again.")).unwrap(),
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

    assert_eq!(bases.base_for("Keys/Piano.fxp"), Some("/surge"));
    assert_eq!(bases.base_for("Dexed_01.syx/00 Say."), Some("/dexed"));
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
        kind_for_patch(&kinds, 0, Some("Keys/Piano.fxp")).unwrap(),
        0
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("Dexed_01.syx/00 Say Again.")).unwrap(),
        1
    );
    assert_eq!(
        kind_for_patch(&kinds, 0, Some("AR Accent Arp.vvp")).unwrap(),
        2
    );
}

/// `.vvp` が `StateFile` に混ざっていると、Surge のインスタンスへ Vaporizer2 の
/// state が流れる。Surge が載っているだけの環境では**引き当てられずにエラー**が正しい。
#[test]
fn a_vvp_patch_is_not_mistaken_for_a_surge_state_file() {
    let kinds = vec![fake_kind("Surge XT", PatchForm::StateFile, None)];

    let error = kind_for_patch(&kinds, 0, Some("PD Emily.vvp")).unwrap_err();

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
        kind_for_patch(&kinds, 0, Some("Keys/Piano.fxp")).unwrap(),
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

    assert_eq!(bases.base_for("Keys/Piano.fxp"), Some("/surge"));
    assert_eq!(bases.base_for("Dexed_01.syx/00 Say."), Some("/dexed"));
    assert_eq!(bases.base_for("AR Accent Arp.vvp"), Some("/vaporizer2"));
}
