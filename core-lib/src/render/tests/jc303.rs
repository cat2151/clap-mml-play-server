//! JC303: 本体の置き場にある `.json` を、activate 前と音色切替の両方で読めること。

use super::*;
use clack_extensions::params::PluginParams;
use cmrt_server_config::JC303_PLUGIN_ID;

use crate::render::jc303_patch::juce_param_id;

const JC303_CLAP_ENV: &str = "CMRT_TEST_JC303_CLAP";

fn factory_preset(name: &str) -> String {
    let root = crate::jc303_preset::jc303_preset_dirs()
        .pop()
        .expect("Documents の置き場が決まらない");
    std::path::Path::new(&root)
        .join("factory")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn renderer(patch: Option<&str>) -> RealtimeRenderer {
    let entry = load_entry(&plugin_path(JC303_CLAP_ENV)).unwrap();
    let config = CoreConfig {
        patch_path: patch.map(wire),
        ..test_config_with_plugin_id(JC303_PLUGIN_ID)
    };
    RealtimeRenderer::new(&config, &entry).unwrap()
}

/// CLAP 上の値（0..1 に正規化）と表示名。
fn param(renderer: &mut RealtimeRenderer, id: &str) -> (f64, String) {
    let instance = renderer.plugin_instance_mut();
    let handle = instance.plugin_handle();
    let params = handle.get_extension::<PluginParams>().unwrap();
    let param_id = juce_param_id(id);
    let value = params.get_value(&handle, param_id).unwrap();
    let mut buffer = [0u8; 256];
    let text = params
        .value_to_text(&handle, param_id, value, &mut buffer)
        .unwrap();
    (value, String::from_utf8_lossy(text).into_owned())
}

fn state_root(renderer: &mut RealtimeRenderer) -> xmltree::Element {
    let state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
    let xml = crate::surge_fx_preset::juce_xml::decode(&state).unwrap();
    xmltree::Element::parse(xml.as_bytes()).unwrap()
}

/// dice_acid.json の値が、parameter 自身の単位のまま届くこと。
fn assert_dice_acid(renderer: &mut RealtimeRenderer) {
    // -1..1 の amount 0.25 は、正規化すると 0.625。
    let (amount, _) = param(renderer, "modSlot3Amount");
    assert!((amount - 0.625).abs() < 1e-6, "modSlot3Amount={amount}");
    assert_eq!(param(renderer, "filterType").1, "Spicy");
    assert_eq!(param(renderer, "lfoDestination").1, "Waveform");
    assert_eq!(param(renderer, "switchModState").1, "On");
    let root = state_root(renderer);
    assert_eq!(root.attributes["modKnob1"], "modSlot3Amount");
    assert_eq!(root.attributes["modKnob2"], "modSlot4Amount");
}

#[test]
#[ignore = "実 JC303 CLAP が要る"]
fn a_preset_is_loaded_before_activation_and_by_switching() {
    let dice_acid = factory_preset("dice_acid.json");
    let mut fresh = renderer(Some(&dice_acid));
    assert_dice_acid(&mut fresh);

    let mut switched = renderer(None);
    switched.set_patch(Some(&wire(&dice_acid))).unwrap();
    assert_dice_acid(&mut switched);
}

/// preset に無い parameter は既定値へ戻る（前の音色の値を引きずらない）。
#[test]
#[ignore = "実 JC303 CLAP が要る"]
fn parameters_missing_from_the_preset_return_to_their_defaults() {
    let mut renderer = renderer(None);
    let default_amount = param(&mut renderer, "modSlot3Amount");
    let default_destination = param(&mut renderer, "lfoDestination");

    renderer
        .set_patch(Some(&wire(&factory_preset("dice_acid.json"))))
        .unwrap();
    assert_ne!(param(&mut renderer, "lfoDestination"), default_destination);
    renderer
        .set_patch(Some(&wire(&factory_preset("init.json"))))
        .unwrap();

    assert_eq!(param(&mut renderer, "modSlot3Amount"), default_amount);
    assert_eq!(param(&mut renderer, "lfoDestination"), default_destination);
}

/// 空回し無しで切り替えた直後の 1 音目が、その音色で作った instance の 1 音目と同じ音になること。
///
/// 切り替える前に音を鳴らすと比べられない。JC303 は同じ音色のままでも 1 音目と 2 音目で波形が
/// 大きく違う（音ごとに内部状態を持ち越す）。
#[test]
#[ignore = "実 JC303 CLAP が要る"]
fn the_first_note_after_switching_sounds_like_a_fresh_load() {
    let squelch = factory_preset("squelch.json");
    let expected = render_live_note(&mut renderer(Some(&squelch)));

    let mut switched = renderer(Some(&factory_preset("hoover.json")));
    switched
        .switch_patch(Some(&wire(&squelch)), true, 0)
        .unwrap();
    let actual = render_live_note(&mut switched);

    let diff = expected
        .iter()
        .zip(&actual)
        .fold(0.0_f32, |max, (a, b)| max.max((a - b).abs()));
    assert!(peak(&expected) > 0.01, "鳴っていない");
    assert!(diff < 1e-4, "最大差 {diff}（peak {}）", peak(&expected));
}

/// 置き場の音色が全部、エラー無く読めること。
#[test]
#[ignore = "実 JC303 CLAP が要る"]
fn every_installed_preset_loads() {
    let mut renderer = renderer(None);
    let mut count = 0;
    for root in crate::jc303_preset::jc303_preset_dirs() {
        for patch in crate::collect_patches(&root).unwrap() {
            let patch = patch.to_string_lossy().into_owned();
            renderer
                .set_patch(Some(&wire(&patch)))
                .unwrap_or_else(|error| panic!("{patch}: {error:#}"));
            count += 1;
        }
    }
    assert!(count > 0, "JC303 の音色が 1 つも無い");
}
