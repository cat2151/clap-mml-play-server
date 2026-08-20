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
