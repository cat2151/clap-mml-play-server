//! Vaporizer2（`com.vastdynamics.VAST2`）の実測と、`.vvp` を CLAP state として
//! 流し込む経路（Stage 2）。
//!
//! `.vvp` を読むテストは、音色置き場を `CMRT_TEST_VAPORIZER2_PRESETS` で渡す。
//! 共通のヘルパと環境変数は親モジュールにある。

use super::*;
use crate::vvp::{read_vvp_header, VAPORIZER2_PLUGIN_ID};

const VAPORIZER2_CLAP_ENV: &str = "CMRT_TEST_VAPORIZER2_CLAP";
const VAPORIZER2_PRESETS_ENV: &str = "CMRT_TEST_VAPORIZER2_PRESETS";
/// 設定すると、耳で確かめたいレンダリング結果をこのディレクトリへ WAV で書き出す。
/// **未設定なら 1 バイトも書かない**（テストが実ユーザーのパスを汚さないため）。
const WAV_OUT_DIR_ENV: &str = "CMRT_TEST_WAV_OUT_DIR";

/// Vaporizer2 が受け入れ条件を通り、かつ MIDI dialect のままであること。
///
/// dialect が MIDI だけということは **NOTE_END による voicing probe が成立しない**
/// （Dexed と同じ）。`.vvp` の `m_uPolyMode` を読む方針はこの実測が根拠。
#[test]
#[ignore = "実プラグインが要る"]
fn vaporizer2_advertises_a_stereo_main_output_and_only_the_midi_note_dialect() {
    let report = probe_plugin_capabilities(&plugin_path(VAPORIZER2_CLAP_ENV), None).unwrap();

    assert_eq!(report.selected.id, "com.vastdynamics.VAST2");
    assert_eq!(report.descriptors.len(), 1);
    assert_eq!(report.audio_output_ports, 1);
    assert_eq!(report.main_output_channels, 2);
    assert!(report.main_output_is_main);
    assert_eq!(report.input_note_ports, 1);
    assert_eq!(report.input_note_dialects, vec!["MIDI".to_string()]);
    assert_eq!(report.rejected, None);
}

/// ADR 0006 を Vaporizer2 へ延長する根拠。**文字列検索ではなく実 probe** で、
/// preset の列挙も読み込みも CLAP の API では出来ないことを押さえる。
#[test]
#[ignore = "実プラグインが要る"]
fn vaporizer2_opts_into_neither_preset_discovery_nor_preset_load() {
    let report = probe_plugin_capabilities(&plugin_path(VAPORIZER2_CLAP_ENV), None).unwrap();

    assert_eq!(report.factories, vec!["clap.plugin-factory".to_string()]);
    assert!(
        !report
            .extensions
            .iter()
            .any(|extension| extension.starts_with("clap.preset-load")),
        "preset-load を opt-in している: {:?}",
        report.extensions
    );
    // state で音色を当てる方針（`.vvp` の XML をそのまま流す）が成立する前提。
    assert!(report.extensions.contains(&"clap.state".to_string()));
}

/// 音色置き場の `.vvp` を `PatchVersion` ごとに 1 つずつ拾う。
///
/// ファイル名を直に書かないのは、音色置き場が個人のものだから。実際にどれが選ばれたかは
/// 失敗時のメッセージに出る。
fn vaporizer2_patches_by_version() -> std::collections::BTreeMap<String, std::path::PathBuf> {
    let dir = std::env::var(VAPORIZER2_PRESETS_ENV).unwrap_or_else(|_| {
        panic!(
            "{VAPORIZER2_PRESETS_ENV} に .vvp のディレクトリを設定してからこのテストを実行すること"
        )
    });
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("音色置き場を読めない '{dir}': {error}"))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "vvp"))
        .collect();
    assert!(!entries.is_empty(), "'{dir}' に .vvp が 1 つも無い");
    entries.sort();

    let mut by_version: std::collections::BTreeMap<String, std::path::PathBuf> =
        std::collections::BTreeMap::new();
    let mut largest = entries[0].clone();
    for path in &entries {
        if std::fs::metadata(path).unwrap().len() > std::fs::metadata(&largest).unwrap().len() {
            largest = path.clone();
        }
        let prefix = std::fs::read(path).unwrap();
        let text = String::from_utf8_lossy(&prefix[..prefix.len().min(4096)]).into_owned();
        let Some(at) = text.find("PatchVersion=\"") else {
            continue;
        };
        let rest = &text[at + "PatchVersion=\"".len()..];
        let version = rest[..rest.find('"').unwrap()].to_string();
        by_version.entry(version).or_insert_with(|| path.clone());
    }
    assert!(
        by_version.len() >= 3,
        "版が 3 種類そろっていない: {:?}",
        by_version.keys().collect::<Vec<_>>()
    );
    // いちばん大きいもの（波形テーブル内蔵。実データで 17MB）も必ず 1 本混ぜる。
    by_version.insert("largest".to_string(), largest);
    by_version
}

/// 版ごとに 1 つずつと、いちばん大きいもの。
fn vaporizer2_sample_patches() -> Vec<std::path::PathBuf> {
    let mut selected: Vec<std::path::PathBuf> =
        vaporizer2_patches_by_version().into_values().collect();
    selected.dedup();
    selected
}

/// 耳で確かめたいものだけ WAV に落とす。[`WAV_OUT_DIR_ENV`] が未設定なら何もしない。
fn dump_wav_for_listening(name: &str, samples: &[f32]) {
    let Ok(dir) = std::env::var(WAV_OUT_DIR_ENV) else {
        return;
    };
    let path = std::path::Path::new(&dir).join(format!("{name}.wav"));
    crate::pipeline::write_wav(samples, SAMPLE_RATE as u32, &path).unwrap();
    eprintln!("wrote {}", path.display());
}

/// `.vvp` の XML を、版の読み替えをせずに JUCE binary-XML で包む。
///
/// `vvp_state_blob` が読み替えなしだったころの挙動そのもの。
fn state_blob_without_retagging(xml: &[u8]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(xml.len() + 9);
    blob.extend_from_slice(&0x2132_4356_u32.to_le_bytes());
    blob.extend_from_slice(&(xml.len() as u32).to_le_bytes());
    blob.extend_from_slice(xml);
    blob.push(0);
    blob
}

/// **Stage 2 の本題**: `.vvp` を CLAP state として流し込むと、版によらず音が出ること。
///
/// 確かめているのは 3 つ:
///
/// 1. どの版でも `load_vvp_patch` がエラーにならず、鳴らすと**無音でない**
/// 2. ロード後にプラグイン自身が保存する state に、**その音色の名前が入っている**
///    （＝要求した音色が本当に載った。サンプル列の比較より直接的）
/// 3. 選んだ音色どうしで**出音が全部違う**（1 つ目が載りっぱなしになっていない）
#[test]
#[ignore = "実プラグインが要る"]
fn vaporizer2_loads_every_patch_version_as_state_and_makes_sound() {
    let path = plugin_path(VAPORIZER2_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let mut renderer =
        RealtimeRenderer::new(&test_config_with_plugin_id(VAPORIZER2_PLUGIN_ID), &entry).unwrap();

    let patches = vaporizer2_sample_patches();
    let mut renders: Vec<(std::path::PathBuf, Vec<f32>)> = Vec::new();
    for patch in &patches {
        let display = patch.display().to_string();
        renderer.load_vvp_patch(&display).unwrap();

        let samples = render_live_note(&mut renderer);
        assert!(peak(&samples) > 0.0, "無音になった: {display}");

        let expected = read_vvp_header(patch).unwrap().name;
        let state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        assert!(
            String::from_utf8_lossy(&state).contains(&expected),
            "載っている音色が '{expected}' ではない: {display}"
        );

        dump_wav_for_listening(
            &patch
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .replace(' ', "_"),
            &samples,
        );
        renders.push((patch.clone(), samples));
    }

    for (index, (path, samples)) in renders.iter().enumerate() {
        for (other_path, other) in &renders[index + 1..] {
            assert!(
                samples != other,
                "出音が同じ: {} と {}",
                path.display(),
                other_path.display()
            );
        }
    }
}

/// 版の読み替え（`V2.00000` → `V2.10000`）が**本当に出音を変えている**こと。
///
/// これが無いと `V2.00000` の 50 件は `externalRepresentation=false` で読まれ、
/// パラメータを誤解釈する。**読み替えても名前も長さも変わらない**ので、
/// 名前の一致やサンプルの非無音では検出できない。読み替えの有無で鳴らし比べる。
#[test]
#[ignore = "実プラグインが要る"]
fn retagging_a_v2_00000_patch_changes_what_it_sounds_like() {
    let legacy = vaporizer2_patches_by_version()
        .remove("VASTVaporizerParamsV2.00000")
        .expect("V2.00000 の音色が音色置き場に無い");
    let path = plugin_path(VAPORIZER2_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let mut renderer =
        RealtimeRenderer::new(&test_config_with_plugin_id(VAPORIZER2_PLUGIN_ID), &entry).unwrap();
    let xml = std::fs::read(&legacy).unwrap();

    super::patch_state::load_plugin_state(
        renderer.plugin_instance_mut(),
        &state_blob_without_retagging(&xml),
    )
    .unwrap();
    let as_written = render_live_note(&mut renderer);
    dump_wav_for_listening("v2_00000_as_written", &as_written);

    renderer
        .load_vvp_patch(&legacy.display().to_string())
        .unwrap();
    let retagged = render_live_note(&mut renderer);
    dump_wav_for_listening("v2_00000_retagged", &retagged);

    assert!(peak(&retagged) > 0.0, "読み替えたら無音になった");
    assert!(
        as_written != retagged,
        "版の読み替えが出音に効いていない: {}",
        legacy.display()
    );
}

/// 逆方向のガード。Surge XT へ `.vvp` の state を流し込むと、無視されるだけでは
/// 済まないかもしれない。**送る前に落とす**（ADR 0007 / 0009 と同型の事故）。
#[test]
#[ignore = "実プラグインが要る"]
fn a_vvp_patch_is_refused_before_it_reaches_surge() {
    let path = plugin_path(SURGE_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let mut renderer = RealtimeRenderer::new(&test_config(), &entry).unwrap();

    let error = renderer.load_vvp_patch("does-not-matter.vvp").unwrap_err();

    assert!(error.to_string().contains(VAPORIZER2_PLUGIN_ID));
}

/// **Stage 3 の本題**: 公開 API の `set_patch()` が `.vvp` を Vaporizer2 の経路へ流すこと。
///
/// Stage 2 の時点では `load_vvp_patch()` を直接呼ぶしか無く、`set_patch()` は `.vvp` を
/// `PatchTarget::StateFile` として扱っていた。そのまま渡すと `load_patch()` が
/// **JUCE の 9 バイトを被せずに生の XML を state へ押し込む**ので、
/// 「操作は成功したのに音色が変わらない」という静かな間違いになる。
/// 音色の名前が state に入るかどうかで、それを見分ける。
#[test]
#[ignore = "実プラグインが要る"]
fn set_patch_routes_a_vvp_path_to_the_vaporizer2_loader() {
    let path = plugin_path(VAPORIZER2_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let mut renderer =
        RealtimeRenderer::new(&test_config_with_plugin_id(VAPORIZER2_PLUGIN_ID), &entry).unwrap();

    for patch in vaporizer2_sample_patches() {
        let display = patch.display().to_string();
        renderer.set_patch(Some(&display)).unwrap();

        let samples = render_live_note(&mut renderer);
        assert!(peak(&samples) > 0.0, "無音になった: {display}");

        let expected = read_vvp_header(&patch).unwrap().name;
        let state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        assert!(
            String::from_utf8_lossy(&state).contains(&expected),
            "set_patch() で載った音色が '{expected}' ではない: {display}"
        );
    }
}

// 生の XML をそのまま state として渡すのでは駄目だ、という Stage 3 の前提は
// **テストとして残せない**。実測すると Vaporizer2 は生の XML を渡された時点で
// STATUS_ACCESS_VIOLATION でプロセスごと落ちる（2026-08-22。テスト
// `the_raw_vvp_xml_is_not_a_valid_clap_state_on_its_own` として一度書いて確認した）。
// テストハーネスごと落ちるので同居させられない。
//
// この実測には設計上の意味がある。`.vvp` を `PatchForm::StateFile` のまま扱うと、
// ADR 0007 が想定していた「静かに間違った音が鳴る」ではなく**プロセスが死ぬ**。
// だから `patch_switch.rs` と `render.rs` の `.vvp` 分岐と、
// `ensure_vvp_capable()` のガードはどちらも省略できない。

/// 起動時の音色（config の `patch_path`）が `.vvp` でも、`activate()` 前のロードが
/// 正しい経路を通ること。ここは `set_patch()` を通らない別の入口。
#[test]
#[ignore = "実プラグインが要る"]
fn a_vvp_patch_in_the_config_is_loaded_before_activate() {
    let path = plugin_path(VAPORIZER2_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let patch = vaporizer2_sample_patches()
        .into_iter()
        .next()
        .expect("音色置き場が空");
    let cfg = CoreConfig {
        patch_path: Some(patch.display().to_string()),
        ..test_config_with_plugin_id(VAPORIZER2_PLUGIN_ID)
    };

    let mut renderer = RealtimeRenderer::new(&cfg, &entry).unwrap();

    let samples = render_live_note(&mut renderer);
    assert!(peak(&samples) > 0.0, "無音になった: {}", patch.display());
    let expected = read_vvp_header(&patch).unwrap().name;
    let state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
    assert!(
        String::from_utf8_lossy(&state).contains(&expected),
        "起動時に載った音色が '{expected}' ではない: {}",
        patch.display()
    );
}

/// 逆方向のガードが `set_patch()` 経由でも効くこと。
/// Surge XT のインスタンスへ `.vvp` を投げても、**送る前に**落ちる。
#[test]
#[ignore = "実プラグインが要る"]
fn set_patch_refuses_a_vvp_path_on_a_surge_instance() {
    let path = plugin_path(SURGE_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let mut renderer = RealtimeRenderer::new(&test_config(), &entry).unwrap();

    let error = renderer.set_patch(Some("does-not-matter.vvp")).unwrap_err();

    assert!(error.to_string().contains(VAPORIZER2_PLUGIN_ID));
}

/// 起動時の音色でも同じガードが効くこと。Surge のインスタンスは `.vvp` を積んで
/// 生成した時点で失敗する（黙って初期音色で立ち上がらない）。
#[test]
#[ignore = "実プラグインが要る"]
fn a_surge_instance_refuses_to_start_with_a_vvp_patch_in_the_config() {
    let path = plugin_path(SURGE_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let cfg = CoreConfig {
        patch_path: Some("does-not-matter.vvp".to_string()),
        ..test_config()
    };

    let error = match RealtimeRenderer::new(&cfg, &entry) {
        Err(error) => error,
        Ok(_) => panic!("Surge のインスタンスが '.vvp' の音色で立ち上がってしまった"),
    };

    assert!(error.to_string().contains(VAPORIZER2_PLUGIN_ID));
}
