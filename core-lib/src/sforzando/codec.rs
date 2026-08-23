use std::io::{Read, Write};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

const MAGIC: &[u8; 4] = b"CEGP";
const HEADER_LEN: usize = 8;

pub(super) fn decode(state: &[u8]) -> anyhow::Result<String> {
    if state.len() < HEADER_LEN {
        anyhow::bail!(
            "Sforzando CEGP header が短い: {} bytes (最低 {HEADER_LEN})",
            state.len()
        );
    }
    if &state[..4] != MAGIC {
        anyhow::bail!(
            "Sforzando state magic が CEGP ではない: {:02X?}",
            &state[..4]
        );
    }
    let expected_len = u32::from_le_bytes(state[4..8].try_into().expect("four bytes")) as usize;
    let mut decoder = ZlibDecoder::new(&state[HEADER_LEN..]);
    let mut xml = Vec::new();
    decoder
        .read_to_end(&mut xml)
        .map_err(|error| anyhow::anyhow!("Sforzando CEGP zlib stream が不正: {error}"))?;
    if xml.len() != expected_len {
        anyhow::bail!(
            "Sforzando CEGP XML length が不一致: header={expected_len} decoded={}",
            xml.len()
        );
    }
    String::from_utf8(xml)
        .map_err(|error| anyhow::anyhow!("Sforzando AriaSave XML が UTF-8 ではない: {error}"))
}

pub(super) fn encode(xml: &[u8]) -> anyhow::Result<Vec<u8>> {
    let xml_len = u32::try_from(xml.len())
        .map_err(|_| anyhow::anyhow!("Sforzando AriaSave XML が u32 length を超えた"))?;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(xml)
        .map_err(|error| anyhow::anyhow!("Sforzando AriaSave XML の zlib 圧縮に失敗: {error}"))?;
    let compressed = encoder
        .finish()
        .map_err(|error| anyhow::anyhow!("Sforzando CEGP zlib stream の完了に失敗: {error}"))?;
    let mut state = Vec::with_capacity(HEADER_LEN + compressed.len());
    state.extend_from_slice(MAGIC);
    state.extend_from_slice(&xml_len.to_le_bytes());
    state.extend_from_slice(&compressed);
    Ok(state)
}
