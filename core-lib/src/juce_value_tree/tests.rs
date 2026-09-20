//! codec の往復。実ファイル（`.t3kpreset`）を使うテストは `#[ignore]` で、
//! 置き場を `CMRT_TEST_TONE3000_PRESETS` で受ける。

use super::*;

fn sample_tree() -> ValueTree {
    ValueTree {
        type_name: "Root".to_string(),
        properties: vec![
            ("int".to_string(), Var::Int(-7)),
            ("yes".to_string(), Var::Bool(true)),
            ("no".to_string(), Var::Bool(false)),
            ("dbl".to_string(), Var::Double(0.25)),
            ("str".to_string(), Var::String("日本語 text".to_string())),
            ("big".to_string(), Var::Int64(1 << 40)),
            ("bin".to_string(), Var::Binary(vec![0, 1, 2, 255])),
            ("void".to_string(), Var::Void),
        ],
        children: vec![
            ValueTree {
                type_name: "Child".to_string(),
                properties: vec![("id".to_string(), Var::String(String::new()))],
                children: Vec::new(),
            },
            ValueTree::default(),
        ],
    }
}

#[test]
fn round_trips_every_var_kind() {
    let tree = sample_tree();
    let bytes = encode(&tree);
    assert_eq!(decode(&bytes).unwrap(), tree);
    assert_eq!(encode(&decode(&bytes).unwrap()), bytes);
}

#[test]
fn compressed_int_uses_the_shortest_little_endian_form() {
    let mut out = Vec::new();
    write_compressed_int(&mut out, 0);
    write_compressed_int(&mut out, 5);
    write_compressed_int(&mut out, 0x1234);
    write_compressed_int(&mut out, -1);
    assert_eq!(out, vec![0, 1, 5, 2, 0x34, 0x12, 0x81, 1]);
}

#[test]
fn string_var_carries_its_nul_terminator_inside_the_length() {
    let mut out = Vec::new();
    write_var(&mut out, &Var::String("ab".to_string()));
    assert_eq!(out, vec![1, 4, MARKER_STRING, b'a', b'b', 0]);
}

#[test]
fn rejects_trailing_bytes() {
    let mut bytes = encode(&sample_tree());
    bytes.push(0);
    let error = decode(&bytes).unwrap_err().to_string();
    assert!(error.contains("余り"), "{error}");
}

#[test]
fn rejects_truncated_input() {
    let bytes = encode(&sample_tree());
    assert!(decode(&bytes[..bytes.len() - 3]).is_err());
}

#[test]
fn property_helpers_replace_in_place() {
    let mut tree = sample_tree();
    tree.set_property("str", Var::String("x".to_string()));
    tree.set_property("new", Var::Int(1));
    assert_eq!(tree.property_string("str"), Some("x"));
    assert_eq!(tree.properties[4].0, "str");
    assert_eq!(tree.properties.last().unwrap().0, "new");
    assert!(tree.child("Child").is_some());
    assert!(tree.child_mut("Nope").is_none());
}

/// TONE3000 の factory preset 7 件が byte 一致で往復する。
#[test]
#[ignore = "実 preset ファイルが要る"]
fn every_t3kpreset_round_trips_byte_exact() {
    let dir = std::env::var("CMRT_TEST_TONE3000_PRESETS")
        .expect("CMRT_TEST_TONE3000_PRESETS に .t3kpreset のディレクトリを設定すること");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("t3kpreset") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"T3KB", "{}", path.display());
        let tree = decode(&bytes[4..]).unwrap_or_else(|error| {
            panic!("{}: {error:#}", path.display());
        });
        assert_eq!(
            encode(&tree),
            &bytes[4..],
            "{} の往復が一致しない",
            path.display()
        );
        assert_eq!(tree.type_name, "T3KPreset");
        assert!(tree
            .property_string("name")
            .is_some_and(|name| !name.is_empty()));
        checked += 1;
    }
    assert!(checked > 0, "{dir} に .t3kpreset が無い");
    eprintln!("round-trip checked: {checked} presets");
}
