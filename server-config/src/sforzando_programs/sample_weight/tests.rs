use std::path::{Path, PathBuf};

use super::*;

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cmrt_test_sfz_sample_weight_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, relative: &str, content: &[u8]) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn counts_samples_through_includes_default_path_and_spaced_names() {
    let dir = fixture_dir("full");
    write(&dir, "Samples/a.wav", &[0; 10]);
    write(&dir, "Samples/b c.wav", &[0; 20]);
    write(&dir, "Samples/Sub/d.wav", &[0; 40]);
    write(&dir, "Other/e.wav", &[0; 80]);
    write(
        &dir,
        "Programs/root.sfz",
        br#"<control> default_path=../Samples/
<region> sample=a.wav lokey=60
<region>sample=b c.wav   hikey=72 // sample=commented.wav
<region> sample=*sine
<region> sample=a.wav
#include "Inc\one.sfz"
"#,
    );
    write(
        &dir,
        "Programs/Inc/one.sfz",
        br#"<region> sample=Sub\d.wav
#include "Inc/two.sfz"
"#,
    );
    write(
        &dir,
        "Programs/Inc/two.sfz",
        br#"<control> default_path=../Other/
<region> sample=e.wav<region> sample=missing.wav
"#,
    );

    let weight = sfz_sample_weight(&dir.join("Programs/root.sfz")).unwrap();

    // a, "b c", Sub/d, e, missing。*sine と重複した a とコメント内は数えない。
    assert_eq!(
        weight,
        SfzSampleWeight {
            files: 5,
            bytes: 10 + 20 + 40 + 80,
            missing: 1,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn circular_includes_stop_at_the_depth_limit() {
    let dir = fixture_dir("circular");
    write(&dir, "x.wav", &[0; 7]);
    write(&dir, "root.sfz", b"#include \"loop.sfz\"\n");
    write(
        &dir,
        "loop.sfz",
        b"<region> sample=x.wav\n#include \"loop.sfz\"\n",
    );

    let weight = sfz_sample_weight(&dir.join("root.sfz")).unwrap();

    assert_eq!(
        weight,
        SfzSampleWeight {
            files: 1,
            bytes: 7,
            missing: 0,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn value_ends_at_hash_and_skips_missing_includes() {
    let dir = fixture_dir("hash");
    write(&dir, "y.wav", &[0; 3]);
    write(
        &dir,
        "root.sfz",
        b"#include \"absent.sfz\"\n<region> sample=y.wav#comment\n",
    );

    let weight = sfz_sample_weight(&dir.join("root.sfz")).unwrap();

    assert_eq!(
        weight,
        SfzSampleWeight {
            files: 1,
            bytes: 3,
            missing: 0,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opcode_names_ending_in_sample_are_not_samples() {
    let dir = fixture_dir("boundary");
    write(&dir, "z.wav", &[0; 5]);
    write(&dir, "root.sfz", b"<region> xsample=z.wav\n");

    let weight = sfz_sample_weight(&dir.join("root.sfz")).unwrap();

    assert_eq!(weight, SfzSampleWeight::default());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn part_sfz_whose_samples_resolve_from_another_directory_misses_all_samples() {
    let dir = fixture_dir("part");
    write(&dir, "Samples/a.wav", &[0; 4]);
    write(
        &dir,
        "Programs/Parts/part.sfz",
        br"<region> sample=..\Samples\a.wav",
    );
    write(&dir, "Programs/top.sfz", b"#include \"Parts/part.sfz\"\n");

    let part = sfz_sample_weight(&dir.join("Programs/Parts/part.sfz")).unwrap();
    let top = sfz_sample_weight(&dir.join("Programs/top.sfz")).unwrap();

    assert!(part.all_samples_missing());
    assert!(!top.all_samples_missing());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn variables_and_builtin_waves_do_not_count_as_missing() {
    let dir = fixture_dir("variables");
    write(
        &dir,
        "root.sfz",
        b"#define $DIR Samples\n<region> sample=$DIR/a.wav\n<region> sample=*saw\n",
    );

    let weight = sfz_sample_weight(&dir.join("root.sfz")).unwrap();

    assert!(!weight.all_samples_missing());
    assert!(!SfzSampleWeight::default().all_samples_missing());
    write(
        &dir,
        "plogue.sfz",
        b"<control> default_path=$sample_dir/Toy/\n<region> sample=a.flac\n",
    );
    let weight = sfz_sample_weight(&dir.join("plogue.sfz")).unwrap();
    assert!(!weight.all_samples_missing());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_root_sfz_is_an_error() {
    let dir = fixture_dir("missing_root");

    assert!(sfz_sample_weight(&dir.join("absent.sfz")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 実在の sfz 置き場で期待値と照合する。`CMRT_SFZ_WEIGHT_DIR` に置き場のパスを入れたときだけ走る。
#[test]
fn matches_expected_weights_in_a_real_sfz_library() {
    let Some(root) = std::env::var_os("CMRT_SFZ_WEIGHT_DIR") else {
        return;
    };
    let root = PathBuf::from(root);
    for (relative, files, megabytes) in [
        ("UI_METAL-GTX/Programs/01-METAL-GTX Full.sfz", 1882, 780.7),
        (
            "UI_Standard_Guitar/Programs/04-Standard Guitar VSOP XTracking.sfz",
            1541,
            296.7,
        ),
        ("VSCO-2-CE-1.1.0/UprightPiano.sfz", 69, 253.6),
    ] {
        let weight = sfz_sample_weight(&root.join(relative)).unwrap();
        let actual_mb = weight.bytes as f64 / 1_000_000.0;
        eprintln!("{relative}: {} files, {actual_mb:.1}MB", weight.files);
        assert!(
            (f64::from(weight.files) - f64::from(files)).abs() <= f64::from(files) * 0.01,
            "{relative}: files {} != {files}",
            weight.files
        );
        assert!(
            (actual_mb - megabytes).abs() <= megabytes * 0.01,
            "{relative}: {actual_mb:.1}MB != {megabytes}MB"
        );
    }
}
