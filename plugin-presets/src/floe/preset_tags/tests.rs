use super::*;

/// 実機の `.floe-preset` と同じく、タグ数の後に「u16 長＋文字列」が並ぶ。
fn preset_with_tags(tags: &[&str]) -> Vec<u8> {
    let mut bytes = b"\x93\x1f\x49\x2a SCC Taiko Drums \x00\x05".to_vec();
    for tag in tags {
        bytes.extend_from_slice(&(tag.len() as u16).to_le_bytes());
        bytes.extend_from_slice(tag.as_bytes());
    }
    bytes.extend_from_slice(b"\x0cSam Windell");
    bytes
}

#[test]
fn a_percussion_tag_is_found_among_the_tags() {
    let preset = preset_with_tags(&["acoustic", "percussion", "ensemble"]);

    assert!(has_tag(&preset, PERCUSSION_TAG));
}

#[test]
fn tonal_percussion_is_not_percussion() {
    let preset = preset_with_tags(&["acoustic", "tonal percussion", "ambient"]);

    assert!(!has_tag(&preset, PERCUSSION_TAG));
}
