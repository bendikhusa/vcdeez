//! Independently reconstructed, dependency-free WLF header and compression v1.
pub const PARCEL: usize = 8192;
pub fn header(data: &[u8]) -> Result<(u16, u16, i32), String> {
    if data.len() < 10 {
        return Err("truncated WLF header".into());
    }
    let magic = u16::from_be_bytes(data[0..2].try_into().unwrap());
    if !matches!(magic, 0x1076 | 0x1364) {
        return Err(format!("unrecognized WLF signature {magic:04x}"));
    }
    let version = u16::from_be_bytes(data[2..4].try_into().unwrap());
    if version != 172 {
        return Err(format!(
            "native header reader currently supports WLF v172, found v{version}"
        ));
    }
    Ok((
        magic,
        version,
        i32::from_be_bytes(data[6..10].try_into().unwrap()),
    ))
}
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    control: u16,
}
impl Bits<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self
            .data
            .get(self.pos)
            .ok_or("truncated WLF compression stream")?;
        self.pos += 1;
        Ok(b)
    }
    fn bit(&mut self) -> Result<u32, String> {
        self.control >>= 1;
        if self.control == 1 {
            self.control = self.byte()? as u16 + 256;
        }
        Ok((self.control & 1) as u32)
    }
}
pub fn decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut bits = Bits {
        data,
        pos: 0,
        control: 2,
    };
    let mut out = Vec::new();
    loop {
        if bits.bit()? == 0 {
            if out.len() == limit {
                return Err("WLF decompression limit exceeded".into());
            }
            out.push(bits.byte()?);
        } else {
            let first = bits.byte()? as u32;
            let second = bits.byte()? as u32;
            let mut offset = ((first & 0xf8) << 5) | second;
            let mut length = first & 7;
            while bits.bit()? != 0 {
                offset = offset.wrapping_mul(2).wrapping_add(bits.bit()?);
            }
            while bits.bit()? != 0 {
                length = length.wrapping_mul(2).wrapping_add(bits.bit()?);
            }
            if offset & 0x80000000 != 0 {
                return Ok(out);
            }
            // Decompiled implementation treats wrapping length+3 == 0 as a no-op.
            let count = length.wrapping_add(3) as usize;
            let distance = offset as usize + 1;
            if count == 0 {
                continue;
            }
            if distance > out.len() {
                return Err("invalid WLF backreference".into());
            }
            if count > limit.saturating_sub(out.len()) {
                return Err("WLF decompression limit exceeded".into());
            }
            for _ in 0..count {
                out.push(out[out.len() - distance]);
            }
        }
    }
}
/// Encode compression-v1 using literal tokens. Valid but deliberately not compact.
pub fn compress_literals(data: &[u8]) -> Vec<u8> {
    struct Writer {
        out: Vec<u8>,
        control: usize,
        count: u8,
    }
    impl Writer {
        fn bit(&mut self, bit: bool) {
            if self.count == 8 {
                self.control = self.out.len();
                self.out.push(0);
                self.count = 0;
            }
            if bit {
                self.out[self.control] |= 1 << self.count;
            }
            self.count += 1;
        }
    }
    let mut writer = Writer {
        out: vec![0],
        control: 0,
        count: 0,
    };
    for byte in data {
        writer.bit(false);
        writer.out.push(*byte);
    }
    writer.bit(true);
    writer.out.extend([0xf8, 0xff]);
    for _ in 0..19 {
        writer.bit(true);
        writer.bit(true);
    }
    writer.bit(false);
    writer.bit(false);
    writer.out
}
pub fn single_parcel_blocks(data: &[u8]) -> Result<Vec<(usize, Vec<u8>)>, String> {
    header(data)?;
    if !data.len().is_multiple_of(PARCEL) {
        return Err("WLF length is not a multiple of 8192".into());
    }
    let mut out = Vec::new();
    for (i, p) in data.as_chunks::<PARCEL>().0.iter().enumerate().skip(1) {
        if p[..2] != [0xff, 0xff] {
            if matches!(
                u16::from_be_bytes(p[..2].try_into().unwrap()),
                0xfffd | 0xfffc | 0xfff9
            ) {
                return Err(format!(
                    "parcel {i}: multi-parcel buffer is not yet supported by native extraction"
                ));
            }
            continue;
        }
        let size = i32::from_be_bytes(p[4..8].try_into().unwrap());
        let len = size.unsigned_abs() as usize;
        if len > PARCEL - 16 {
            return Err(format!(
                "parcel {i}: buffer spans multiple parcels; native extraction not yet implemented"
            ));
        }
        let block = if size > 0 {
            decompress(&p[16..16 + len], 64 * 1024 * 1024)?
        } else {
            p[16..16 + len].to_vec()
        };
        out.push((i, block));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoder_rejects_invalid_streams() {
        let compressed = include_bytes!("../vsim.wlf");
        assert!(decompress(&[], 1024).is_err());
        assert!(decompress(&compressed[0xa010..0xa047], 1).is_err());
    }
    #[test]
    fn real_sample_blocks() {
        let blocks = single_parcel_blocks(include_bytes!("../vsim.wlf")).unwrap();
        assert_eq!(
            blocks.iter().map(|(_, b)| b.len()).collect::<Vec<_>>(),
            [390, 11123, 38, 9135]
        );
    }
    #[test]
    fn literal_encoder_roundtrip() {
        for raw in [
            b"".as_slice(),
            b"abc".as_slice(),
            include_bytes!("../vsim.wlf").as_slice(),
        ] {
            assert_eq!(decompress(&compress_literals(raw), raw.len()).unwrap(), raw);
        }
    }
}
