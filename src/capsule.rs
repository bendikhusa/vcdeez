use crate::hash::sha256;
use std::fmt::Write;

const PREFIX: &str = "$comment vcdeez.wlf.v1 ";

pub fn embed(vcd: &str, wlf: &[u8]) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "{PREFIX}header {} {} {} $end",
        wlf.len(),
        sha256(wlf),
        sha256(vcd.as_bytes())
    )
    .unwrap();
    for (i, chunk) in wlf.chunks(1024).enumerate() {
        write!(out, "{PREFIX}chunk {i} ").unwrap();
        for b in chunk {
            write!(out, "{b:02x}").unwrap();
        }
        out.push_str(" $end\n");
    }
    out.push_str(vcd);
    out
}

pub fn restore(vcd: &str) -> Result<Vec<u8>, String> {
    let mut header = None;
    let mut bytes = Vec::new();
    let mut body = String::new();
    let mut chunks = 0usize;
    for line in vcd.split_inclusive('\n') {
        if let Some(value) = line.strip_prefix(PREFIX) {
            let words: Vec<_> = value.split_whitespace().collect();
            match words.as_slice() {
                ["header", size, wlf_hash, vcd_hash, "$end"] if header.is_none() && chunks == 0 => {
                    let size = size.parse::<usize>().map_err(|_| "invalid capsule size")?;
                    if size > vcd.len() / 2 {
                        return Err("capsule size exceeds available input".into());
                    }
                    header = Some((size, wlf_hash.to_string(), vcd_hash.to_string()));
                }
                ["chunk", index, hex, "$end"] if header.is_some() => {
                    if index.parse::<usize>().ok() != Some(chunks) {
                        return Err("missing or reordered capsule chunk".into());
                    }
                    if hex.len() % 2 != 0 || hex.len() > 2048 {
                        return Err("invalid capsule hex length".into());
                    }
                    for pair in hex.as_bytes().as_chunks::<2>().0 {
                        let digit = |x: u8| match x {
                            b'0'..=b'9' => Ok(x - b'0'),
                            b'a'..=b'f' => Ok(x - b'a' + 10),
                            _ => Err("invalid capsule hex"),
                        };
                        bytes.push(digit(pair[0])? * 16 + digit(pair[1])?);
                    }
                    chunks += 1;
                }
                _ => return Err("malformed or duplicate reconstruction metadata".into()),
            }
        } else {
            body.push_str(line);
        }
    }
    let (size, wlf_hash, vcd_hash) = header.ok_or(
        "VCD has no vcdeez reconstruction capsule; ordinary VCD cannot recover WLF record types",
    )?;
    if bytes.len() != size || sha256(&bytes) != wlf_hash {
        return Err("WLF capsule is incomplete or corrupt".into());
    }
    if sha256(body.as_bytes()) != vcd_hash {
        return Err("VCD content was edited; refusing to restore stale WLF data".into());
    }
    crate::native::header(&bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Vec<u8> {
        let mut b = vec![0; 8192];
        b[..10].copy_from_slice(&[0x10, 0x76, 0, 0xac, 0, 100, 0, 0, 0, 6]);
        b
    }
    #[test]
    fn exact_recovery_and_edit_rejection() {
        let raw = sample();
        let vcd = embed("$timescale 1ns $end\n#0\n0!\n", &raw);
        assert_eq!(restore(&vcd).unwrap(), raw);
        assert!(restore(&vcd.replace("0!", "1!"))
            .unwrap_err()
            .contains("edited"));
        assert!(restore(&vcd.replacen("chunk 0", "chunk 1", 1)).is_err());
        assert!(restore(&vcd.replacen("1076", "1077", 1)).is_err());
    }
}
