//! DEFLATE decompressor (RFC 1951) and zlib wrapper (RFC 1950), from scratch.
//!
//! Needed twice over: the texture pack is a ZIP of deflated entries, and every
//! texture inside it is a PNG whose IDAT is a zlib stream. One decoder serves both.

/// Reads bits least-significant-first, the order DEFLATE uses.
struct BitReader<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            byte: 0,
            bit: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u32, String> {
        let byte = *self
            .data
            .get(self.byte)
            .ok_or_else(|| "deflate: input ended mid-stream".to_string())?;
        let value = (byte >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.byte += 1;
        }
        Ok(value as u32)
    }

    fn read_bits(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0;
        for i in 0..count {
            value |= self.read_bit()? << i;
        }
        Ok(value)
    }

    fn align_to_byte(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.byte += 1;
        }
    }
}

/// Canonical Huffman table: codes are assigned in order of increasing length,
/// so decoding only needs the count per length and the symbols in that order.
struct Huffman {
    /// `counts[len]` = how many codes have this bit length.
    counts: [u16; 16],
    /// Symbols sorted by (code length, symbol).
    symbols: Vec<u16>,
}

impl Huffman {
    fn from_lengths(lengths: &[u8]) -> Huffman {
        let mut counts = [0u16; 16];
        for &len in lengths {
            counts[len as usize] += 1;
        }
        counts[0] = 0;

        let mut offsets = [0u16; 16];
        for len in 1..16 {
            offsets[len] = offsets[len - 1] + counts[len - 1];
        }

        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &len) in lengths.iter().enumerate() {
            if len != 0 {
                symbols[offsets[len as usize] as usize] = symbol as u16;
                offsets[len as usize] += 1;
            }
        }
        Huffman { counts, symbols }
    }

    fn decode(&self, reader: &mut BitReader) -> Result<u16, String> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..16 {
            code |= reader.read_bit()? as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("deflate: invalid Huffman code".to_string())
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// Order in which code-length code lengths appear in a dynamic block header.
const CLEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn fixed_tables() -> (Huffman, Huffman) {
    let mut lit_lengths = [0u8; 288];
    for (i, len) in lit_lengths.iter_mut().enumerate() {
        *len = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    (Huffman::from_lengths(&lit_lengths), Huffman::from_lengths(&[5u8; 30]))
}

fn read_dynamic_tables(reader: &mut BitReader) -> Result<(Huffman, Huffman), String> {
    let hlit = reader.read_bits(5)? as usize + 257;
    let hdist = reader.read_bits(5)? as usize + 1;
    let hclen = reader.read_bits(4)? as usize + 4;

    let mut clen_lengths = [0u8; 19];
    for &slot in CLEN_ORDER.iter().take(hclen) {
        clen_lengths[slot] = reader.read_bits(3)? as u8;
    }
    let clen_table = Huffman::from_lengths(&clen_lengths);

    // Literal/length and distance lengths share one run-length encoded list.
    let mut lengths = vec![0u8; hlit + hdist];
    let mut i = 0;
    while i < lengths.len() {
        let symbol = clen_table.decode(reader)?;
        match symbol {
            0..=15 => {
                lengths[i] = symbol as u8;
                i += 1;
            }
            16 => {
                let prev = *lengths
                    .get(i.wrapping_sub(1))
                    .ok_or_else(|| "deflate: repeat with no previous length".to_string())?;
                let repeat = 3 + reader.read_bits(2)? as usize;
                for _ in 0..repeat {
                    if i >= lengths.len() {
                        return Err("deflate: length repeat overflows table".to_string());
                    }
                    lengths[i] = prev;
                    i += 1;
                }
            }
            17 | 18 => {
                let repeat = if symbol == 17 {
                    3 + reader.read_bits(3)? as usize
                } else {
                    11 + reader.read_bits(7)? as usize
                };
                i = (i + repeat).min(lengths.len());
            }
            _ => return Err("deflate: invalid code-length symbol".to_string()),
        }
    }

    Ok((
        Huffman::from_lengths(&lengths[..hlit]),
        Huffman::from_lengths(&lengths[hlit..]),
    ))
}

/// Decompress a raw DEFLATE stream (no zlib or gzip wrapper).
pub fn inflate_raw(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut reader = BitReader::new(data);
    let mut out: Vec<u8> = Vec::with_capacity(data.len() * 4);

    loop {
        let last = reader.read_bit()? == 1;
        let block_type = reader.read_bits(2)?;
        match block_type {
            0 => {
                reader.align_to_byte();
                let start = reader.byte;
                if start + 4 > data.len() {
                    return Err("deflate: truncated stored block header".to_string());
                }
                let len = u16::from_le_bytes([data[start], data[start + 1]]) as usize;
                let end = start + 4 + len;
                if end > data.len() {
                    return Err("deflate: truncated stored block".to_string());
                }
                out.extend_from_slice(&data[start + 4..end]);
                reader.byte = end;
            }
            1 | 2 => {
                let (lit_table, dist_table) = if block_type == 1 {
                    fixed_tables()
                } else {
                    read_dynamic_tables(&mut reader)?
                };
                loop {
                    let symbol = lit_table.decode(&mut reader)?;
                    match symbol {
                        0..=255 => out.push(symbol as u8),
                        256 => break,
                        257..=285 => {
                            let idx = symbol as usize - 257;
                            let length = LENGTH_BASE[idx] as usize
                                + reader.read_bits(LENGTH_EXTRA[idx] as u32)? as usize;
                            let dsym = dist_table.decode(&mut reader)? as usize;
                            if dsym >= DIST_BASE.len() {
                                return Err("deflate: invalid distance symbol".to_string());
                            }
                            let distance = DIST_BASE[dsym] as usize
                                + reader.read_bits(DIST_EXTRA[dsym] as u32)? as usize;
                            if distance > out.len() {
                                return Err("deflate: distance before start of output".to_string());
                            }
                            // Copies may overlap (that is how runs are encoded),
                            // so this has to go byte by byte.
                            let from = out.len() - distance;
                            for k in 0..length {
                                let byte = out[from + k];
                                out.push(byte);
                            }
                        }
                        _ => return Err("deflate: invalid literal/length symbol".to_string()),
                    }
                }
            }
            _ => return Err("deflate: reserved block type".to_string()),
        }
        if last {
            break;
        }
    }
    Ok(out)
}

/// Decompress a zlib stream (RFC 1950): 2-byte header, deflate data, Adler-32.
pub fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 6 {
        return Err("zlib: stream too short".to_string());
    }
    let (cmf, flg) = (data[0], data[1]);
    if cmf & 0x0F != 8 {
        return Err(format!("zlib: unsupported compression method {}", cmf & 0x0F));
    }
    if (u16::from_be_bytes([cmf, flg])) % 31 != 0 {
        return Err("zlib: header check failed".to_string());
    }
    if flg & 0x20 != 0 {
        return Err("zlib: preset dictionaries are not supported".to_string());
    }
    let out = inflate_raw(&data[2..])?;

    let checksum = u32::from_be_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]);
    if crate::png::adler32(&out) != checksum {
        return Err("zlib: Adler-32 mismatch".to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_stored_blocks() {
        // The encoder in png.rs emits stored blocks; the decoder must read them back.
        let raw: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let mut zlib = vec![0x78, 0x01, 0x01];
        zlib.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(&raw);
        zlib.extend_from_slice(&crate::png::adler32(&raw).to_be_bytes());
        assert_eq!(inflate_zlib(&zlib).unwrap(), raw);
    }

    #[test]
    fn decodes_a_fixed_huffman_block() {
        // "abc" as a fixed-Huffman deflate stream, wrapped in zlib.
        let zlib = [0x78, 0x9c, 0x4b, 0x4c, 0x4a, 0x06, 0x00, 0x02, 0x4d, 0x01, 0x27];
        assert_eq!(inflate_zlib(&zlib).unwrap(), b"abc");
    }

    #[test]
    fn rejects_a_corrupt_header() {
        assert!(inflate_zlib(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).is_err());
    }
}
