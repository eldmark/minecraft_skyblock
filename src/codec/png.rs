//! PNG encoding (and, later, decoding) written from scratch on std only.
//!
//! The encoder emits a zlib stream made of uncompressed ("stored") deflate blocks.
//! That is a valid, if bulky, PNG: every viewer reads it, and it needs no compressor.

const CRC_TABLE: [u32; 256] = build_crc_table();

const fn build_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Wrap raw bytes in a zlib stream of stored deflate blocks (max 65535 bytes each).
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // deflate, 32K window, no preset dict, check bits
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    }
    let mut offset = 0;
    while offset < raw.len() {
        let len = (raw.len() - offset).min(0xFFFF);
        let last = offset + len == raw.len();
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&(len as u16).to_le_bytes());
        out.extend_from_slice(&(!(len as u16)).to_le_bytes());
        out.extend_from_slice(&raw[offset..offset + len]);
        offset += len;
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// Encode 8-bit RGB pixels (row-major, `width * height * 3` bytes) as a PNG file.
pub fn encode_rgb(width: usize, height: usize, rgb: &[u8]) -> Vec<u8> {
    assert_eq!(rgb.len(), width * height * 3, "pixel buffer size mismatch");

    // Every scanline is prefixed with filter type 0 (None).
    let mut raw = Vec::with_capacity(height * (1 + width * 3));
    for y in 0..height {
        raw.push(0);
        raw.extend_from_slice(&rgb[y * width * 3..(y + 1) * width * 3]);
    }

    let mut out = Vec::with_capacity(raw.len() + 1024);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // depth 8, truecolor, deflate, adaptive, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_known_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn adler32_matches_the_known_check_value() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn encoder_and_decoder_round_trip() {
        // Filter 0 rows and stored blocks on the way out, Paeth-free parsing
        // on the way back in: this pins both halves against each other.
        let mut rgb = Vec::new();
        for y in 0..17u32 {
            for x in 0..23u32 {
                rgb.extend_from_slice(&[(x * 11) as u8, (y * 15) as u8, ((x + y) * 7) as u8]);
            }
        }
        let png = encode_rgb(23, 17, &rgb);
        let image = decode(&png).unwrap();
        assert_eq!((image.width, image.height), (23, 17));
        for i in 0..23 * 17 {
            assert_eq!(&image.rgba[i * 4..i * 4 + 3], &rgb[i * 3..i * 3 + 3]);
            assert_eq!(image.rgba[i * 4 + 3], 255);
        }
    }

    #[test]
    fn decoder_rejects_a_non_png() {
        assert!(decode(b"not a png at all").is_err());
    }

    #[test]
    fn encoded_png_has_the_expected_structure() {
        let png = encode_rgb(2, 2, &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

use crate::codec::inflate::inflate_zlib;

/// An 8-bit RGBA image.
#[derive(Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// `width * height * 4` bytes, row-major, top row first.
    pub rgba: Vec<u8>,
}

/// Undo PNG's per-scanline filters. `bpp` is bytes per pixel, rounded up.
fn unfilter(raw: &[u8], width: usize, height: usize, bpp: usize) -> Result<Vec<u8>, String> {
    let stride = width * bpp;
    if raw.len() < height * (stride + 1) {
        return Err("png: IDAT shorter than the declared image".to_string());
    }
    let mut out = vec![0u8; height * stride];
    for y in 0..height {
        let filter = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..y * (stride + 1) + 1 + stride];
        for x in 0..stride {
            let a = if x >= bpp { out[y * stride + x - bpp] } else { 0 }; // left
            let b = if y > 0 { out[(y - 1) * stride + x] } else { 0 }; // up
            let c = if x >= bpp && y > 0 {
                out[(y - 1) * stride + x - bpp]
            } else {
                0
            }; // up-left
            let value = match filter {
                0 => line[x],
                1 => line[x].wrapping_add(a),
                2 => line[x].wrapping_add(b),
                3 => line[x].wrapping_add((((a as u16) + (b as u16)) / 2) as u8),
                4 => line[x].wrapping_add(paeth(a, b, c)),
                other => return Err(format!("png: unknown filter type {other}")),
            };
            out[y * stride + x] = value;
        }
    }
    Ok(out)
}

/// PNG's Paeth predictor: pick whichever neighbour the gradient points at.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Decode an 8-bit PNG (grayscale, RGB, palette, or either with alpha) to RGBA.
/// Interlaced and 16-bit images are rejected: the texture pack contains none.
pub fn decode(data: &[u8]) -> Result<Image, String> {
    if data.len() < 8 || data[..8] != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Err("png: bad signature".to_string());
    }

    let mut pos = 8;
    let (mut width, mut height, mut color_type) = (0usize, 0usize, 0u8);
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut alpha_palette: Vec<u8> = Vec::new();
    let mut idat = Vec::new();

    while pos + 8 <= data.len() {
        let len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let kind = &data[pos + 4..pos + 8];
        let body_start = pos + 8;
        let body_end = body_start + len;
        if body_end + 4 > data.len() {
            return Err("png: truncated chunk".to_string());
        }
        let body = &data[body_start..body_end];

        match kind {
            b"IHDR" => {
                if len < 13 {
                    return Err("png: short IHDR".to_string());
                }
                width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
                color_type = body[9];
                if body[8] != 8 {
                    return Err(format!("png: unsupported bit depth {}", body[8]));
                }
                if body[12] != 0 {
                    return Err("png: interlaced images are not supported".to_string());
                }
            }
            b"PLTE" => {
                palette = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => alpha_palette = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos = body_end + 4; // skip the chunk CRC
    }

    if width == 0 || height == 0 {
        return Err("png: missing or empty IHDR".to_string());
    }

    let channels = match color_type {
        0 => 1, // grayscale
        2 => 3, // truecolor
        3 => 1, // palette index
        4 => 2, // grayscale + alpha
        6 => 4, // truecolor + alpha
        other => return Err(format!("png: unsupported color type {other}")),
    };

    let raw = inflate_zlib(&idat)?;
    let samples = unfilter(&raw, width, height, channels)?;

    let mut rgba = vec![255u8; width * height * 4];
    for i in 0..width * height {
        let s = &samples[i * channels..i * channels + channels];
        let out = &mut rgba[i * 4..i * 4 + 4];
        match color_type {
            0 => out[..3].copy_from_slice(&[s[0], s[0], s[0]]),
            2 => out[..3].copy_from_slice(&s[..3]),
            3 => {
                let index = s[0] as usize;
                let color = *palette
                    .get(index)
                    .ok_or_else(|| "png: palette index out of range".to_string())?;
                out[..3].copy_from_slice(&color);
                out[3] = *alpha_palette.get(index).unwrap_or(&255);
            }
            4 => {
                out[..3].copy_from_slice(&[s[0], s[0], s[0]]);
                out[3] = s[1];
            }
            _ => out.copy_from_slice(&s[..4]),
        }
    }

    Ok(Image {
        width,
        height,
        rgba,
    })
}
