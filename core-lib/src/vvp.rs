//! Vaporizer2 の音色ファイル（`.vvp`）と、そこから作る CLAP state。
//!
//! 単位は Surge XT の `.fxp` と同じ「1 音色 = 1 ファイル = 1 CLAP state」。違うのは
//! 中身が **UTF-8 の XML そのもの**だという点で、`.vvp` を書く `savePatchXML` と
//! CLAP の `getStateInformation` が同じ `createPatchXML(true)` を通っている
//! （`VASTAudioProcessor.cpp:576` と `:934`）。したがって JUCE の binary-XML ヘッダを
//! 被せるだけで、既存の `clap.state` ロード経路にそのまま乗る。
//!
//! CLAP には preset を列挙・選択する専用 API（preset-discovery factory と
//! `clap.preset-load` extension）があるが、**Vaporizer2 3.5.0 はどちらも持たない**
//! （実 probe。`docs/adr/0006-no-generic-clap-preset-api.md`）。列挙も選択も host 側でやる。
//!
//! # なぜ Program Change ではないか
//! Vaporizer2 にも program で選ぶ経路はあるが、プラグインが起動時に非同期スキャンした
//! preset 配列の index に依存する（`setCurrentProgram` → `loadPreset(index)`）。
//! 順序が環境依存なうえ、`setChunk` 直後 400ms のガードもある（`:542`）。
//! Dexed で同じ形の罠を踏んでいる（`docs/adr/0003-dexed-program-change-guard.md`）ので採らない。

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

/// `.vvp` を音色置き場にする唯一の既知プラグインの CLAP plugin ID。
///
/// ロード経路が「patch 文字列の形」で決まる（`docs/adr/0007-patch-string-decides-the-plugin.md`）
/// 以上、**選ばれた形と実際に載っているプラグインが食い違っていないか**をロード直前に
/// 照合する必要がある。照合しないと、Surge XT のインスタンスへ Vaporizer2 の state を
/// 流し込んで「操作は成功したのに音が変わらない（あるいは壊れる）」ことになる。
///
/// `cmrt_server_config` 側にも同じ定数がある（config の `active_plugin` 解決用）。
/// [`crate::dx7::DEXED_PLUGIN_ID`] と同じ理由で二重に持っている。
pub const VAPORIZER2_PLUGIN_ID: &str = "com.vastdynamics.VAST2";

/// JUCE の `copyXmlToBinary` が付ける magic（`juce_AudioProcessor.cpp:946-961`）。
const JUCE_BINARY_XML_MAGIC: u32 = 0x2132_4356;

/// magic 4 バイト + XML 長 4 バイト。
const JUCE_BINARY_XML_HEADER_LEN: usize = 8;

/// ヘッダを読むために先頭から読むバイト数。
///
/// 必要な情報（版・名前・カテゴリ・`m_uPolyMode`）はすべて先頭にある。**460 ファイル
/// 全読み（681MB、最大 1 ファイル 17MB）は絶対にしない。** 実測での `m_uPolyMode` の
/// 終端は最大 835 バイト目（`AR Comb ARP.vvp`）だが、`PatchComments` が長い
/// ユーザープリセットでは後ろへずれるので余裕を持たせてある。
const HEADER_PREFIX_LEN: u64 = 4096;

/// `setStateInformation` が `externalRepresentation=false` で読んでしまう版。
const LEGACY_PATCH_VERSION: &[u8] = b"VASTVaporizerParamsV2.00000";

/// 上を読み替える先。ファイルの表現（external）と解釈が一致する。
const RETAGGED_PATCH_VERSION: &[u8] = b"VASTVaporizerParamsV2.10000";

// バイト列の置換なので長さが変わってはいけない。
const _: () = assert!(LEGACY_PATCH_VERSION.len() == RETAGGED_PATCH_VERSION.len());

/// `m_uPolyMode` が Mono のときの値。これ以外（`Poly4` / `Poly16` / `Poly32`）は和音が鳴る。
const MONO_POLY_MODE: &str = "Mono";

/// Vaporizer2 の音色ファイルの拡張子。
const VVP_EXTENSION: &str = ".vvp";

const PATH_SEPARATORS: [char; 2] = ['/', '\\'];

/// patch path が Vaporizer2 の音色を指しているか。
///
/// [`crate::is_cartridge_patch_path`] と対になる判定で、材料は文字列の形だけ
/// （`docs/adr/0007-patch-string-decides-the-plugin.md`）。`.syx` 側と同じく
/// **どのコンポーネントに現れても真**にしてある。`.vvp` は 1 ファイル = 1 音色なので
/// 末尾コンポーネントだけを見ても足りるが、規則を 2 種類にすると
/// 「`.syx` は途中でもよいが `.vvp` は末尾だけ」という覚え方が要るので揃える。
pub fn is_vvp_patch_path(patch: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(has_vvp_extension)
}

fn has_vvp_extension(component: &str) -> bool {
    component.len() > VVP_EXTENSION.len()
        && component[component.len() - VVP_EXTENSION.len()..].eq_ignore_ascii_case(VVP_EXTENSION)
}

/// `.vvp` の XML を、そのまま `clap.state` へ流せるバイト列にする。
///
/// JUCE の binary-XML は次の形（`juce_AudioProcessor.cpp:946-961`）:
///
/// ```text
/// [0..4)  u32 LE = 0x21324356
/// [4..8)  u32 LE = XML のバイト長（末尾 NUL を含まない）
/// [8..)   UTF-8 XML 本文
/// 末尾     0x00 を 1 バイト
/// ```
///
/// 版の読み替え（[`retag_legacy_patch_version`]）もここで行う。
pub fn vvp_state_blob(xml: &[u8]) -> Vec<u8> {
    let xml = retag_legacy_patch_version(xml);
    let mut blob = Vec::with_capacity(xml.len() + JUCE_BINARY_XML_HEADER_LEN + 1);
    blob.extend_from_slice(&JUCE_BINARY_XML_MAGIC.to_le_bytes());
    blob.extend_from_slice(&(xml.len() as u32).to_le_bytes());
    blob.extend_from_slice(&xml);
    blob.push(0);
    blob
}

/// `V2.00000` と名乗る XML を `V2.10000` へ読み替える。
///
/// `setStateInformation` は **`V2.00000` のときだけ `externalRepresentation=false`** で
/// パースする（`VASTAudioProcessor.cpp:954-955`。コード中のコメント自体が疑問形）。
/// しかしファイルとして保存された `.vvp` は常に external 表現なので、そのまま流すと
/// **パラメータを誤解釈する**。`V2.10000` にすると解釈がファイルの表現と一致し、
/// skew 補正は `2.00000` と `2.10000` の両方に掛かる（`:1339-1340`）ので失われない。
///
/// **`V2.20000` には触らない**（skew の挙動が違う）。
fn retag_legacy_patch_version(xml: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    let Some(at) = find_bytes(xml, LEGACY_PATCH_VERSION) else {
        return std::borrow::Cow::Borrowed(xml);
    };
    let mut retagged = xml.to_vec();
    // 実データでは 1 ファイルに 1 回しか出てこないが、根拠にはしない。
    let mut from = at;
    while let Some(offset) = find_bytes(&retagged[from..], LEGACY_PATCH_VERSION) {
        let at = from + offset;
        retagged[at..at + RETAGGED_PATCH_VERSION.len()].copy_from_slice(RETAGGED_PATCH_VERSION);
        from = at + RETAGGED_PATCH_VERSION.len();
    }
    std::borrow::Cow::Owned(retagged)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 一覧に載せるために要る、`.vvp` の先頭から読めるメタデータ。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VvpHeader {
    pub name: String,
    /// `PatchCategory`。2 文字コード（`AR` / `BA` / `PD` …）。
    pub category: String,
    pub tag: String,
    pub author: String,
    /// `m_uPolyMode` が `Mono` 以外か。**和音行の候補にできるのはこれが `true` のものだけ。**
    pub poly: bool,
}

/// `.vvp` の先頭だけを読んでメタデータを取る。
pub fn read_vvp_header(path: &Path) -> Result<VvpHeader> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("音色ファイルを開けない '{}'", path.display()))?;
    let mut prefix = Vec::new();
    file.take(HEADER_PREFIX_LEN)
        .read_to_end(&mut prefix)
        .with_context(|| format!("音色ファイルを読めない '{}'", path.display()))?;
    parse_vvp_header(&prefix).with_context(|| format!("'{}'", path.display()))
}

/// `.vvp` の先頭バイト列からメタデータを取る。
///
/// 途中で切れたバイト列を渡してよい（[`read_vvp_header`] がそうする）。ただし
/// `m_uPolyMode` まで届いていないと **Mono を poly と誤って和音行へ出してしまう**ので、
/// 見つからないときは黙って既定値にせずエラーにする。
pub fn parse_vvp_header(prefix: &[u8]) -> Result<VvpHeader> {
    let text = String::from_utf8_lossy(prefix);
    if !text.contains("<VASTvaporizer2") {
        anyhow::bail!("Vaporizer2 の音色ファイルではない（先頭に '<VASTvaporizer2' が無い）");
    }
    let poly_mode = param_text(&text, "m_uPolyMode").ok_or_else(|| {
        anyhow::anyhow!(
            "先頭 {HEADER_PREFIX_LEN} バイトに 'm_uPolyMode' が無い（和音で鳴らせるか判定できない）"
        )
    })?;
    Ok(VvpHeader {
        name: attribute(&text, "PatchName").unwrap_or_default(),
        category: attribute(&text, "PatchCategory").unwrap_or_default(),
        tag: attribute(&text, "PatchTag").unwrap_or_default(),
        author: attribute(&text, "PatchAuthor").unwrap_or_default(),
        poly: poly_mode != MONO_POLY_MODE,
    })
}

/// `<PARAM id="..." text="..."/>` の `text` を読む。
fn param_text(text: &str, id: &str) -> Option<String> {
    let at = text.find(&format!("id=\"{id}\""))?;
    let rest = &text[at..];
    // 別の PARAM の text を拾わないよう、この要素の終わりまでに限る。
    let element_end = rest.find("/>")?;
    quoted_value(&rest[..element_end], "text=\"")
}

/// 開始タグの属性値を読む。
fn attribute(text: &str, name: &str) -> Option<String> {
    quoted_value(text, &format!("{name}=\""))
}

fn quoted_value(text: &str, prefix: &str) -> Option<String> {
    let at = text.find(prefix)? + prefix.len();
    let rest = &text[at..];
    let end = rest.find('"')?;
    Some(decode_xml_entities(&rest[..end]))
}

/// 属性値に現れる XML の実体参照を戻す。
///
/// 実データでは `CustomModulatorNText` にしか出てこないが、名前や作者に `&` が入った
/// ユーザープリセットで一覧の表示が壊れるのを避けるため、読む値すべてに掛ける。
fn decode_xml_entities(value: &str) -> String {
    if !value.contains('&') {
        return value.to_string();
    }
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        // `&amp;` は最後。先に戻すと `&amp;lt;` が `<` になってしまう。
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests;
