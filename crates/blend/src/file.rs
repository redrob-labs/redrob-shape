//! File container: header, compression, block table.

use crate::{BlendError, Result};
use std::io::Read;

/// Parsed file header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub pointer_size: usize,
    pub little_endian: bool,
    /// Blender version, e.g. 257, 405, 501.
    pub version: u32,
    /// 0 = legacy 12-byte header, 1 = 17-byte header (Blender 5.0+, 64-bit block lengths).
    pub format: u8,
    pub len: usize,
}

/// One file block: a 4-char code, the struct it holds, how many, and where its bytes are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub code: [u8; 4],
    /// Address the data had in Blender's memory when written; pointers in other blocks refer to it.
    pub old: u64,
    pub sdna_index: usize,
    pub count: usize,
    pub start: usize,
    pub end: usize,
}

impl Block {
    pub fn code_str(&self) -> String {
        self.code.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect()
    }
    /// ID blocks have two-letter codes (`OB`, `ME`, `MA`...) followed by zero bytes; `DATA`,
    /// `DNA1`, `ENDB`, `GLOB`, `REND`, `TEST`, `USER` are not IDs.
    pub fn is_id(&self) -> bool {
        self.code[2] == 0 && self.code[0].is_ascii_uppercase() && self.code[1].is_ascii_uppercase()
    }
    pub fn id_code(&self) -> &[u8] {
        &self.code[..2]
    }
}

/// Decompress zstd (Blender 3.0+) or gzip (older) files; pass raw files through.
pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.starts_with(b"BLENDER") {
        return Ok(bytes.to_vec());
    }
    if bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        let mut out = Vec::new();
        // Blender writes a seekable multi-frame stream; the streaming decoder walks every frame
        // and skips the seek-table skippable frame.
        let mut decoder = zstd::stream::Decoder::new(bytes).map_err(|e| BlendError::Compression(e.to_string()))?;
        decoder.read_to_end(&mut out).map_err(|e| BlendError::Compression(e.to_string()))?;
        return Ok(out);
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut out).map_err(|e| BlendError::Compression(e.to_string()))?;
        return Ok(out);
    }
    Err(BlendError::NotABlendFile)
}

pub fn parse_header(bytes: &[u8]) -> Result<Header> {
    if bytes.len() < 12 || &bytes[..7] != b"BLENDER" {
        return Err(BlendError::NotABlendFile);
    }
    let digits = |s: &[u8]| -> Option<u32> { std::str::from_utf8(s).ok()?.parse().ok() };
    match bytes[7] {
        b'-' | b'_' => {
            let pointer_size = if bytes[7] == b'-' { 8 } else { 4 };
            let little_endian = match bytes[8] {
                b'v' => true,
                b'V' => false,
                _ => return Err(BlendError::Header("endian marker")),
            };
            let version = digits(&bytes[9..12]).ok_or(BlendError::Header("version digits"))?;
            Ok(Header { pointer_size, little_endian, version, format: 0, len: 12 })
        }
        b'0'..=b'9' => {
            let size = digits(&bytes[7..9]).ok_or(BlendError::Header("header size"))? as usize;
            if size != 17 || bytes.len() < 17 || bytes[9] != b'-' || bytes[12] != b'v' {
                return Err(BlendError::Header("unknown new-style header"));
            }
            let format = digits(&bytes[10..12]).ok_or(BlendError::Header("format version"))? as u8;
            if format != 1 {
                return Err(BlendError::Header("file format version"));
            }
            let version = digits(&bytes[13..17]).ok_or(BlendError::Header("version digits"))?;
            Ok(Header { pointer_size: 8, little_endian: true, version, format: 1, len: 17 })
        }
        _ => Err(BlendError::Header("pointer size marker")),
    }
}

/// Little-endian/big-endian aware integer reads.
#[derive(Clone, Copy, Debug)]
pub struct Endian(pub bool);

impl Endian {
    pub fn i16(self, b: &[u8]) -> i16 {
        let a = [b[0], b[1]];
        if self.0 { i16::from_le_bytes(a) } else { i16::from_be_bytes(a) }
    }
    pub fn i32(self, b: &[u8]) -> i32 {
        let a = [b[0], b[1], b[2], b[3]];
        if self.0 { i32::from_le_bytes(a) } else { i32::from_be_bytes(a) }
    }
    pub fn u32(self, b: &[u8]) -> u32 {
        self.i32(b) as u32
    }
    pub fn i64(self, b: &[u8]) -> i64 {
        let a: [u8; 8] = b[..8].try_into().expect("8 bytes");
        if self.0 { i64::from_le_bytes(a) } else { i64::from_be_bytes(a) }
    }
    pub fn u64(self, b: &[u8]) -> u64 {
        self.i64(b) as u64
    }
    pub fn f32(self, b: &[u8]) -> f32 {
        f32::from_bits(self.u32(b))
    }
    pub fn pointer(self, b: &[u8], size: usize) -> u64 {
        if size == 8 { self.u64(b) } else { self.u32(b) as u64 }
    }
}

/// Walk the block headers after the file header until `ENDB`.
pub fn parse_blocks(bytes: &[u8], header: &Header) -> Result<Vec<Block>> {
    let e = Endian(header.little_endian);
    let mut blocks = Vec::new();
    let mut pos = header.len;
    loop {
        let (code, old, sdna_index, count, len, head_len) = match (header.format, header.pointer_size) {
            (0, 4) => {
                need(bytes, pos, 20)?;
                let b = &bytes[pos..];
                ([b[0], b[1], b[2], b[3]], e.u32(&b[8..]) as u64, e.i32(&b[12..]), e.i32(&b[16..]) as i64, e.i32(&b[4..]) as i64, 20)
            }
            (0, _) => {
                need(bytes, pos, 24)?;
                let b = &bytes[pos..];
                ([b[0], b[1], b[2], b[3]], e.u64(&b[8..]), e.i32(&b[16..]), e.i32(&b[20..]) as i64, e.i32(&b[4..]) as i64, 24)
            }
            _ => {
                need(bytes, pos, 32)?;
                let b = &bytes[pos..];
                ([b[0], b[1], b[2], b[3]], e.u64(&b[8..]), e.i32(&b[4..]), e.i64(&b[24..]), e.i64(&b[16..]), 32)
            }
        };
        if &code == b"ENDB" {
            break;
        }
        if len < 0 || sdna_index < 0 || count < 0 {
            return Err(BlendError::Block(pos, "negative size"));
        }
        let start = pos + head_len;
        let end = start + len as usize;
        if end > bytes.len() {
            return Err(BlendError::Block(pos, "block runs past the end of the file"));
        }
        blocks.push(Block { code, old, sdna_index: sdna_index as usize, count: count as usize, start, end });
        pos = end;
    }
    Ok(blocks)
}

fn need(bytes: &[u8], pos: usize, n: usize) -> Result<()> {
    if pos + n > bytes.len() { Err(BlendError::Block(pos, "truncated block header")) } else { Ok(()) }
}
