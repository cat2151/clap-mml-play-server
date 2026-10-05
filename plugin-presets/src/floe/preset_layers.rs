//! Floe CodeState の versioned binary layout から layer の参照と入力鍵域だけを読む。
//! 仕様: https://github.com/floe-audio/Floe/blob/main/src/common_infrastructure/state/state_coding.cpp

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};

use super::library_regions::library_hash;

#[derive(Debug)]
pub(super) enum Instrument {
    None,
    Sampler { library: u64, id: String },
    Waveform,
}

#[derive(Debug)]
pub(super) struct Layer {
    pub instrument: Instrument,
    pub low: u8,
    pub high: u8,
    pub transpose: i16,
}

pub(super) fn read(bytes: &[u8]) -> Result<Vec<Layer>> {
    let mut reader = Reader(bytes);
    if reader.u32()? != 0x2a491f93 {
        bail!("Floe preset の magic が不正");
    }
    let version = reader.u16()?;
    // 1..38 use the same prefix through the parameter block. Reject future layout changes.
    if !(1..=38).contains(&version) {
        bail!("Floe preset state version が非対応: {version}");
    }
    if version >= 3 {
        reader.take(4)?;
    }
    let mut layers = Vec::new();
    for _ in 0..3 {
        let instrument = match reader.u8()? {
            0 => Instrument::None,
            1 => {
                let library = if version >= 9 {
                    reader.u64()?
                } else if version >= 5 {
                    library_hash(&reader.string32()?)
                } else {
                    let author = reader.string32()?;
                    let name = reader.string32()?;
                    if author == "FrozenPlain (Mirage)" {
                        library_hash(&format!("{name} - FrozenPlain - OG"))
                    } else {
                        library_hash(&format!("{name} - {author}"))
                    }
                };
                Instrument::Sampler {
                    library,
                    id: reader.string32()?,
                }
            }
            2..=4 => Instrument::Waveform,
            kind => bail!("Floe preset の instrument type が不正: {kind}"),
        };
        if version >= 2 {
            let points = reader.u8()?;
            if points > 8 {
                bail!("Floe preset の velocity curve が不正");
            }
            reader.take(usize::from(points) * 12)?;
        }
        if version >= 8 {
            reader.take(16)?; // HarmonyIntervalsBitset: 97 bits, two u64s.
        }
        if version >= 14 {
            reader.take(64 * 8)?; // 64 arp steps: u16,u16,u8,u8,s8,u8.
        }
        if version >= 15 {
            reader.take(2)?; // SliceArpConfig: two u8s.
        }
        layers.push(Layer {
            instrument,
            low: 0,
            high: 127,
            transpose: 0,
        });
    }
    let tags = reader.u8()?;
    for _ in 0..tags {
        reader.string16()?;
    }
    for _ in 0..3 {
        reader.string16()?; // author, description, instance id.
    }
    let count = reader.u16()?;
    let mut params = BTreeMap::new();
    for _ in 0..count {
        let id = reader.u32()?;
        let value = f32::from_le_bytes(reader.take(4)?.try_into()?);
        params.insert(id, value);
    }
    for (index, layer) in layers.iter_mut().enumerate() {
        let base = 160 * (index as u32 + 1);
        layer.transpose = parameter(&params, base + 48, 0, -36, 36)?;
        if version >= 4 {
            layer.low = parameter(&params, base + 50, 0, 0, 127)? as u8;
            layer.high = parameter(&params, base + 51, 127, 0, 127)? as u8;
            // Floe normalises inverted input bounds at playback.
            layer.high = layer.high.max(layer.low);
        }
    }
    Ok(layers)
}

fn parameter(
    params: &BTreeMap<u32, f32>,
    id: u32,
    default: i16,
    low: i16,
    high: i16,
) -> Result<i16> {
    let Some(value) = params.get(&id) else {
        return Ok(default);
    };
    if !value.is_finite() || *value < f32::from(low) || *value > f32::from(high) {
        bail!("Floe preset の鍵域 parameter が不正: id={id}, value={value}");
    }
    Ok(value.round() as i16)
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let bytes = self.0.get(..n).context("Floe preset のデータが途切れた")?;
        self.0 = &self.0[n..];
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
    fn string16(&mut self) -> Result<String> {
        let n = usize::from(self.u16()?);
        Ok(std::str::from_utf8(self.take(n)?)?.to_owned())
    }
    fn string32(&mut self) -> Result<String> {
        let n = usize::try_from(self.u32()?)?;
        Ok(std::str::from_utf8(self.take(n)?)?.to_owned())
    }
}
