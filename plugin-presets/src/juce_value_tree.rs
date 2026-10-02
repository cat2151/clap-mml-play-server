//! JUCE `ValueTree::writeToStream` / `readFromStream` のバイナリ形式。
//!
//! TONE3000 の preset（`.t3kpreset`）と plugin state は、4 byte の magic のあとに
//! この形式で木を 1 本流している。読みと書きが byte 単位で往復すること
//! （`encode(decode(x)) == x`）を保証するので、plugin が保存した state を template
//! にして一部だけ差し替えられる。
//!
//! ```text
//! tree   := string(type) cint(nprops) { string(name) var }* cint(nchildren) tree*
//! string := UTF-8 bytes + 0x00
//! cint   := 1 byte n（bit7 = 負）+ n byte little-endian
//! var    := cint(len) [ marker byte + payload(len-1) ]   len == 0 は void
//! ```
//!
//! string var（marker 5）の payload は UTF-8 + 0x00 で、NUL を含めた長さが len に入る。

use anyhow::{bail, Context, Result};

/// JUCE `var` のうち、preset / state に現れる型。
#[derive(Clone, Debug, PartialEq)]
pub enum Var {
    Void,
    Int(i32),
    Bool(bool),
    Double(f64),
    String(String),
    Int64(i64),
    Binary(Vec<u8>),
}

const MARKER_INT: u8 = 1;
const MARKER_BOOL_TRUE: u8 = 2;
const MARKER_BOOL_FALSE: u8 = 3;
const MARKER_DOUBLE: u8 = 4;
const MARKER_STRING: u8 = 5;
const MARKER_INT64: u8 = 6;
const MARKER_BINARY: u8 = 8;

/// JUCE `ValueTree` の 1 ノード。property は書かれた順を保つ（往復一致のため）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValueTree {
    pub type_name: String,
    pub properties: Vec<(String, Var)>,
    pub children: Vec<ValueTree>,
}

impl ValueTree {
    pub fn property(&self, name: &str) -> Option<&Var> {
        self.properties
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    pub fn property_string(&self, name: &str) -> Option<&str> {
        match self.property(name)? {
            Var::String(value) => Some(value.as_str()),
            _ => None,
        }
    }

    /// 既存の property は値だけ差し替え、無ければ末尾へ足す。
    pub fn set_property(&mut self, name: &str, value: Var) {
        match self.properties.iter_mut().find(|(key, _)| key == name) {
            Some(slot) => slot.1 = value,
            None => self.properties.push((name.to_string(), value)),
        }
    }

    pub fn child(&self, type_name: &str) -> Option<&ValueTree> {
        self.children
            .iter()
            .find(|child| child.type_name == type_name)
    }

    pub fn child_mut(&mut self, type_name: &str) -> Option<&mut ValueTree> {
        self.children
            .iter_mut()
            .find(|child| child.type_name == type_name)
    }
}

/// バイト列全体を 1 本の木として読む。末尾に余りがあればエラー。
pub fn decode(bytes: &[u8]) -> Result<ValueTree> {
    let mut reader = Reader { bytes, pos: 0 };
    let tree = reader.tree()?;
    if reader.pos != bytes.len() {
        bail!(
            "ValueTree の末尾に {} byte の余りがある（木は {} byte で終わった）",
            bytes.len() - reader.pos,
            reader.pos
        );
    }
    Ok(tree)
}

pub fn encode(tree: &ValueTree) -> Vec<u8> {
    let mut out = Vec::new();
    write_tree(&mut out, tree);
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, len: usize) -> Result<&[u8]> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "ValueTree が途中で切れている (offset {} で {} byte 必要)",
                    self.pos,
                    len
                )
            })?;
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn compressed_int(&mut self) -> Result<i64> {
        let header = self.take(1)?[0];
        let negative = header & 0x80 != 0;
        let count = usize::from(header & 0x7f);
        if count > 4 {
            bail!("compressed int の長さが {count} byte（最大 4）");
        }
        let mut value: i64 = 0;
        for (index, byte) in self.take(count)?.iter().enumerate() {
            value |= i64::from(*byte) << (8 * index);
        }
        Ok(if negative { -value } else { value })
    }

    fn count(&mut self) -> Result<usize> {
        let value = self.compressed_int()?;
        usize::try_from(value).map_err(|_| anyhow::anyhow!("負の要素数 {value}"))
    }

    fn cstring(&mut self) -> Result<String> {
        let rest = &self.bytes[self.pos..];
        let len = rest
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| anyhow::anyhow!("NUL 終端の無い文字列 (offset {})", self.pos))?;
        let text = std::str::from_utf8(&rest[..len])
            .context("ValueTree の文字列が UTF-8 ではない")?
            .to_string();
        self.pos += len + 1;
        Ok(text)
    }

    fn var(&mut self) -> Result<Var> {
        let len = self.count()?;
        if len == 0 {
            return Ok(Var::Void);
        }
        let payload = self.take(len)?;
        let (marker, body) = (payload[0], &payload[1..]);
        Ok(match marker {
            MARKER_INT => Var::Int(i32::from_le_bytes(fixed(body)?)),
            MARKER_BOOL_TRUE => Var::Bool(true),
            MARKER_BOOL_FALSE => Var::Bool(false),
            MARKER_DOUBLE => Var::Double(f64::from_le_bytes(fixed(body)?)),
            MARKER_STRING => {
                let Some((0, text)) = body.split_last() else {
                    bail!("ValueTree の string var が NUL で終わっていない");
                };
                Var::String(
                    std::str::from_utf8(text)
                        .context("ValueTree の string var が UTF-8 ではない")?
                        .to_string(),
                )
            }
            MARKER_INT64 => Var::Int64(i64::from_le_bytes(fixed(body)?)),
            MARKER_BINARY => Var::Binary(body.to_vec()),
            other => bail!("未対応の var marker {other}"),
        })
    }

    fn tree(&mut self) -> Result<ValueTree> {
        let type_name = self.cstring()?;
        let property_count = self.count()?;
        let mut properties = Vec::with_capacity(property_count);
        for _ in 0..property_count {
            let name = self.cstring()?;
            let value = self.var()?;
            properties.push((name, value));
        }
        let child_count = self.count()?;
        let mut children = Vec::with_capacity(child_count);
        for _ in 0..child_count {
            children.push(self.tree()?);
        }
        Ok(ValueTree {
            type_name,
            properties,
            children,
        })
    }
}

fn fixed<const N: usize>(body: &[u8]) -> Result<[u8; N]> {
    body.try_into()
        .map_err(|_| anyhow::anyhow!("var の payload が {} byte（期待 {N}）", body.len()))
}

fn write_compressed_int(out: &mut Vec<u8>, value: i64) {
    let mut magnitude = value.unsigned_abs();
    let mut data = Vec::with_capacity(4);
    while magnitude > 0 {
        data.push((magnitude & 0xff) as u8);
        magnitude >>= 8;
    }
    let mut header = data.len() as u8;
    if value < 0 {
        header |= 0x80;
    }
    out.push(header);
    out.extend_from_slice(&data);
}

fn write_cstring(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(text.as_bytes());
    out.push(0);
}

fn write_var(out: &mut Vec<u8>, value: &Var) {
    let (marker, body): (u8, Vec<u8>) = match value {
        Var::Void => {
            write_compressed_int(out, 0);
            return;
        }
        Var::Int(value) => (MARKER_INT, value.to_le_bytes().to_vec()),
        Var::Bool(true) => (MARKER_BOOL_TRUE, Vec::new()),
        Var::Bool(false) => (MARKER_BOOL_FALSE, Vec::new()),
        Var::Double(value) => (MARKER_DOUBLE, value.to_le_bytes().to_vec()),
        Var::String(value) => {
            let mut body = value.as_bytes().to_vec();
            body.push(0);
            (MARKER_STRING, body)
        }
        Var::Int64(value) => (MARKER_INT64, value.to_le_bytes().to_vec()),
        Var::Binary(value) => (MARKER_BINARY, value.clone()),
    };
    write_compressed_int(out, (body.len() + 1) as i64);
    out.push(marker);
    out.extend_from_slice(&body);
}

fn write_tree(out: &mut Vec<u8>, tree: &ValueTree) {
    write_cstring(out, &tree.type_name);
    write_compressed_int(out, tree.properties.len() as i64);
    for (name, value) in &tree.properties {
        write_cstring(out, name);
        write_var(out, value);
    }
    write_compressed_int(out, tree.children.len() as i64);
    for child in &tree.children {
        write_tree(out, child);
    }
}

#[cfg(test)]
mod tests;
