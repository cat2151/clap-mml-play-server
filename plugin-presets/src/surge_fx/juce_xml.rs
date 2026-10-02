//! JUCE `AudioProcessor::copyXmlToBinary` / `getXmlFromBinary` の形式。
//!
//! ```text
//! 4 byte magic "VC2!" + 4 byte LE 長（XML 本文の byte 数）+ XML 本文 + 0x00
//! ```

use anyhow::{bail, Result};

const MAGIC: &[u8; 4] = b"VC2!";

/// バイナリ state から XML 本文を取り出す。
pub fn decode(bytes: &[u8]) -> Result<String> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        bail!("JUCE XML state の magic が VC2! ではない");
    }
    let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let body = bytes
        .get(8..8 + length)
        .ok_or_else(|| anyhow::anyhow!("JUCE XML state が長さ {length} より短い"))?;
    Ok(String::from_utf8_lossy(body).into_owned())
}

/// XML 本文をバイナリ state にする。
pub fn encode(xml: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(xml.len() + 9);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(xml.len() as u32).to_le_bytes());
    out.extend_from_slice(xml.as_bytes());
    out.push(0);
    out
}
