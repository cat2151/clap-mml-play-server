use super::*;

/// 実データと同じ形の最小ヘッダ。`version` と `poly_mode` だけ差し替えられる。
fn vvp_xml(version: &str, poly_mode: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n\
         <VASTvaporizer2 PatchVersion=\"{version}\" PatchName=\"Accent Arp\"\r\n\
         \x20               PatchCategory=\"AR\" PatchTag=\"Factory\" PatchAuthor=\"VASTDynamics\"\r\n\
         \x20               PatchComments=\"\" CustomModulator1Text=\"LPF\">\r\n\
         \x20 <PARAM id=\"m_fMasterVolumedB\" text=\"0\"/>\r\n\
         \x20 <PARAM id=\"m_uPolyMode\" text=\"{poly_mode}\"/>\r\n\
         \x20 <PARAM id=\"m_bLegatoMode\" text=\"0\"/>\r\n\
         </VASTvaporizer2>\r\n"
    )
    .into_bytes()
}

#[test]
fn the_state_blob_wraps_the_xml_in_the_juce_binary_xml_header() {
    let xml = vvp_xml("VASTVaporizerParamsV2.20000", "Poly16");

    let blob = vvp_state_blob(&xml);

    assert_eq!(&blob[0..4], &JUCE_BINARY_XML_MAGIC.to_le_bytes());
    assert_eq!(
        u32::from_le_bytes(blob[4..8].try_into().unwrap()) as usize,
        xml.len(),
        "長さは末尾 NUL を含まない"
    );
    assert_eq!(&blob[8..blob.len() - 1], &xml[..]);
    assert_eq!(*blob.last().unwrap(), 0);
    assert_eq!(blob.len(), xml.len() + 9);
}

/// V2.00000 をそのまま流すと `externalRepresentation=false` で読まれてパラメータが
/// 狂う。50 件が該当するので、ここが消えると音が静かに壊れる。
#[test]
fn a_v2_00000_patch_is_retagged_to_v2_10000() {
    let xml = vvp_xml("VASTVaporizerParamsV2.00000", "Poly16");

    let blob = vvp_state_blob(&xml);
    let body = String::from_utf8(blob[8..blob.len() - 1].to_vec()).unwrap();

    assert!(body.contains("VASTVaporizerParamsV2.10000"));
    assert!(!body.contains("VASTVaporizerParamsV2.00000"));
    assert_eq!(
        blob.len(),
        xml.len() + 9,
        "読み替えは同じ長さでなければならない"
    );
}

/// V2.20000 は skew の挙動が違うので触ってはいけない。
#[test]
fn a_v2_20000_patch_is_left_alone() {
    let xml = vvp_xml("VASTVaporizerParamsV2.20000", "Poly16");

    let blob = vvp_state_blob(&xml);
    let body = String::from_utf8(blob[8..blob.len() - 1].to_vec()).unwrap();

    assert!(body.contains("VASTVaporizerParamsV2.20000"));
}

#[test]
fn a_v2_10000_patch_is_left_alone() {
    let xml = vvp_xml("VASTVaporizerParamsV2.10000", "Poly16");

    let blob = vvp_state_blob(&xml);
    let body = String::from_utf8(blob[8..blob.len() - 1].to_vec()).unwrap();

    assert!(body.contains("VASTVaporizerParamsV2.10000"));
}

#[test]
fn the_header_reads_the_name_category_tag_and_author() {
    let header = parse_vvp_header(&vvp_xml("VASTVaporizerParamsV2.20000", "Poly16")).unwrap();

    assert_eq!(
        header,
        VvpHeader {
            name: "Accent Arp".to_string(),
            category: "AR".to_string(),
            tag: "Factory".to_string(),
            author: "VASTDynamics".to_string(),
            poly: true,
        }
    );
}

/// 和音行の候補を絞る唯一の材料。Mono を poly と読むと、和音行に当てた音色が
/// 最後の 1 音しか鳴らない。
#[test]
fn only_mono_reads_as_not_poly() {
    for (poly_mode, expected) in [
        ("Mono", false),
        ("Poly4", true),
        ("Poly16", true),
        ("Poly32", true),
    ] {
        let header = parse_vvp_header(&vvp_xml("VASTVaporizerParamsV2.20000", poly_mode)).unwrap();

        assert_eq!(header.poly, expected, "m_uPolyMode = {poly_mode}");
    }
}

/// 先頭だけ読む以上、途中で切れたバイト列が渡る。`m_uPolyMode` まで届いていないなら
/// **黙って poly 扱いにせずエラーにする**（Mono が和音行へ出るほうが困る）。
#[test]
fn a_prefix_that_stops_before_the_poly_mode_is_an_error() {
    let xml = vvp_xml("VASTVaporizerParamsV2.20000", "Poly16");
    let cut = find_bytes(&xml, b"m_uPolyMode").unwrap();

    let error = parse_vvp_header(&xml[..cut]).unwrap_err();

    assert!(error.to_string().contains("m_uPolyMode"));
}

#[test]
fn a_file_that_is_not_a_vaporizer2_patch_is_an_error() {
    let error = parse_vvp_header(b"CcnK\0\0\0\0FPCh").unwrap_err();

    assert!(error.to_string().contains("VASTvaporizer2"));
}

/// `id="..."` の直後の `text="..."` だけを読む。次の PARAM まで探しに行くと、
/// 属性が無い要素で 1 つ後ろの値を拾ってしまう。
#[test]
fn the_poly_mode_does_not_leak_from_the_next_param() {
    let xml = b"<VASTvaporizer2 PatchName=\"X\">\
                <PARAM id=\"m_uPolyMode\"/>\
                <PARAM id=\"m_bLegatoMode\" text=\"Mono\"/>";

    let error = parse_vvp_header(xml).unwrap_err();

    assert!(error.to_string().contains("m_uPolyMode"));
}

#[test]
fn attribute_values_decode_xml_entities() {
    let xml = b"<VASTvaporizer2 PatchName=\"A &amp; B &gt; C\" PatchAuthor=\"&quot;me&quot;\">\
                <PARAM id=\"m_uPolyMode\" text=\"Poly16\"/>";

    let header = parse_vvp_header(xml).unwrap();

    assert_eq!(header.name, "A & B > C");
    assert_eq!(header.author, "\"me\"");
}

/// `&amp;lt;` を `<` にしてはいけない（実体参照そのものを書いた名前が壊れる）。
#[test]
fn a_double_escaped_entity_only_unescapes_once() {
    assert_eq!(decode_xml_entities("&amp;lt;"), "&lt;");
}

/// `.vvp` を含むディレクトリ名で誤判定しないこと。判定材料は各コンポーネントの末尾だけ。
#[test]
fn a_directory_named_like_a_patch_does_not_make_every_path_a_vvp() {
    assert!(is_vvp_patch_path("Bank.vvp/BA Sub.vvp"));
    assert!(!is_vvp_patch_path("vvp/Pad 1.fxp"));
    assert!(!is_vvp_patch_path("Vaporizer2 Presets/Pad 1.fxp"));
}
