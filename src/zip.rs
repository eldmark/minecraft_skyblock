//! Minimal ZIP reader, enough to pull texture entries out of a resource pack.
//!
//! Reads the central directory (so entry names are known without scanning the
//! whole file), then decompresses one entry at a time. Supports the only two
//! methods resource packs use: stored (0) and deflate (8).

use std::collections::HashMap;
use std::path::Path;

use crate::inflate::inflate_raw;

const END_OF_CENTRAL_DIR: u32 = 0x0605_4B50;
const CENTRAL_FILE_HEADER: u32 = 0x0201_4B50;

struct Entry {
    method: u16,
    compressed_size: usize,
    uncompressed_size: usize,
    local_header_offset: usize,
}

pub struct ZipArchive {
    data: Vec<u8>,
    entries: HashMap<String, Entry>,
}

fn u16_at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

impl ZipArchive {
    pub fn open(path: &Path) -> Result<ZipArchive, String> {
        let data = std::fs::read(path).map_err(|e| format!("zip: cannot read {}: {e}", path.display()))?;
        Self::from_bytes(data)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<ZipArchive, String> {
        // The end-of-central-directory record sits in the last 64KB + 22 bytes,
        // after a comment of unknown length, so it is found by scanning backwards.
        let eocd = (0..data.len().saturating_sub(21))
            .rev()
            .find(|&i| u32_at(&data, i) == END_OF_CENTRAL_DIR)
            .ok_or_else(|| "zip: end-of-central-directory record not found".to_string())?;

        let count = u16_at(&data, eocd + 10) as usize;
        let mut offset = u32_at(&data, eocd + 16) as usize;

        let mut entries = HashMap::with_capacity(count);
        for _ in 0..count {
            if offset + 46 > data.len() || u32_at(&data, offset) != CENTRAL_FILE_HEADER {
                break;
            }
            let method = u16_at(&data, offset + 10);
            let compressed_size = u32_at(&data, offset + 20) as usize;
            let uncompressed_size = u32_at(&data, offset + 24) as usize;
            let name_len = u16_at(&data, offset + 28) as usize;
            let extra_len = u16_at(&data, offset + 30) as usize;
            let comment_len = u16_at(&data, offset + 32) as usize;
            let local_header_offset = u32_at(&data, offset + 42) as usize;

            let name = String::from_utf8_lossy(&data[offset + 46..offset + 46 + name_len]).into_owned();
            if !name.ends_with('/') {
                entries.insert(
                    name,
                    Entry {
                        method,
                        compressed_size,
                        uncompressed_size,
                        local_header_offset,
                    },
                );
            }
            offset += 46 + name_len + extra_len + comment_len;
        }

        Ok(ZipArchive { data, entries })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(|s| s.as_str())
    }

    /// Decompress one entry by its full path inside the archive.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| format!("zip: entry not found: {name}"))?;

        // The local header repeats the name and extra field with its own lengths,
        // which may differ from the central directory's, so re-read them here.
        let head = entry.local_header_offset;
        if head + 30 > self.data.len() {
            return Err(format!("zip: truncated local header for {name}"));
        }
        let name_len = u16_at(&self.data, head + 26) as usize;
        let extra_len = u16_at(&self.data, head + 28) as usize;
        let start = head + 30 + name_len + extra_len;
        let end = start + entry.compressed_size;
        if end > self.data.len() {
            return Err(format!("zip: truncated data for {name}"));
        }
        let body = &self.data[start..end];

        let out = match entry.method {
            0 => body.to_vec(),
            8 => inflate_raw(body).map_err(|e| format!("zip: {name}: {e}"))?,
            other => return Err(format!("zip: unsupported compression method {other} for {name}")),
        };
        if entry.uncompressed_size != 0 && out.len() != entry.uncompressed_size {
            return Err(format!(
                "zip: {name} inflated to {} bytes, expected {}",
                out.len(),
                entry.uncompressed_size
            ));
        }
        Ok(out)
    }
}
