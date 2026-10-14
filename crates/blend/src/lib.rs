//! Read-only `.blend` importer.
//!
//! A `.blend` is a list of blocks, each tagged with the DNA struct it holds; the `DNA1` block
//! carries the struct catalogue, so a file can be read without knowing Blender's headers at
//! compile time. [`BlendFile`] parses that container; [`import`] walks objects, meshes, materials,
//! lights and cameras into a [`Scene`]. Blender is Z-up; the scene comes out Y-up.
//!
//! Supported: Blender 2.5x through 5.x, uncompressed, gzip or zstd; meshes as modern attribute
//! layers (`position` / `.corner_vert` / face offsets), as MPoly/MLoop, or as legacy MFace.

pub mod dna;
pub mod file;
mod import;

pub use file::{Block, Header};
pub use import::import;

use dna::{Sdna, View};
use std::collections::HashMap;
use std::path::Path;

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-blend";

#[derive(Debug, thiserror::Error)]
pub enum BlendError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a .blend file")]
    NotABlendFile,
    #[error("decompression failed: {0}")]
    Compression(String),
    #[error("bad header: {0}")]
    Header(&'static str),
    #[error("bad block at byte {0}: {1}")]
    Block(usize, &'static str),
    #[error("no DNA1 block")]
    NoDna,
    #[error("DNA: {0}")]
    Dna(String),
    #[error("mesh: {0}")]
    Mesh(#[from] shape_geom::MeshError),
}

pub type Result<T> = std::result::Result<T, BlendError>;

/// A parsed file: bytes, block table and struct catalogue.
pub struct BlendFile {
    pub bytes: Vec<u8>,
    pub header: Header,
    pub blocks: Vec<Block>,
    pub sdna: Sdna,
    /// First block index for each old address.
    by_address: HashMap<u64, usize>,
}

impl BlendFile {
    pub fn open(path: &Path) -> Result<Self> {
        Self::parse(&std::fs::read(path)?)
    }

    pub fn parse(raw: &[u8]) -> Result<Self> {
        let bytes = file::decompress(raw)?;
        let header = file::parse_header(&bytes)?;
        let blocks = file::parse_blocks(&bytes, &header)?;
        let dna_block = blocks.iter().find(|b| &b.code == b"DNA1").ok_or(BlendError::NoDna)?;
        let sdna = Sdna::parse(&bytes[dna_block.start..dna_block.end], header.pointer_size, file::Endian(header.little_endian))?;
        let mut by_address = HashMap::new();
        for (i, b) in blocks.iter().enumerate() {
            by_address.entry(b.old).or_insert(i);
        }
        Ok(Self { bytes, header, blocks, sdna, by_address })
    }

    pub fn version(&self) -> u32 {
        self.header.version
    }

    /// Blocks with a given ID code (`OB`, `ME`, `MA`, `LA`, `CA`...).
    pub fn ids(&self, code: &[u8; 2]) -> impl Iterator<Item = (usize, &Block)> {
        self.blocks.iter().enumerate().filter(move |(_, b)| b.is_id() && b.id_code() == code)
    }

    /// Resolve a pointer written from inside block `from`. Modern files reuse DATA addresses
    /// across IDs, so the blocks that follow `from` up to the next ID block are searched first.
    pub fn resolve(&self, pointer: u64, from: usize) -> Option<usize> {
        if pointer == 0 {
            return None;
        }
        for (i, b) in self.blocks.iter().enumerate().skip(from + 1) {
            if b.is_id() || &b.code == b"DNA1" {
                break;
            }
            if b.old == pointer {
                return Some(i);
            }
        }
        self.by_address.get(&pointer).copied()
    }

    pub fn data(&self, block: usize) -> &[u8] {
        let b = &self.blocks[block];
        &self.bytes[b.start..b.end]
    }

    /// The `n`th struct instance in a block, typed by the block's own SDNA index.
    pub fn view(&self, block: usize, n: usize) -> Option<View<'_>> {
        let b = &self.blocks[block];
        let size = self.sdna.struct_size(b.sdna_index);
        let start = b.start + n * size;
        (start + size <= b.end).then(|| View { sdna: &self.sdna, struct_index: b.sdna_index, bytes: &self.bytes[start..start + size] })
    }

    /// Instances in a block interpreted as `struct_name`, regardless of what the block claims
    /// (DATA blocks often carry arrays of a known struct).
    pub fn views_as<'a>(&'a self, block: usize, struct_name: &str) -> Vec<View<'a>> {
        let Some(si) = self.sdna.struct_index(struct_name) else { return Vec::new() };
        let size = self.sdna.struct_size(si);
        let b = &self.blocks[block];
        if size == 0 {
            return Vec::new();
        }
        (0..(b.end - b.start) / size)
            .map(|n| View { sdna: &self.sdna, struct_index: si, bytes: &self.bytes[b.start + n * size..b.start + (n + 1) * size] })
            .collect()
    }

    pub fn struct_name_of(&self, block: usize) -> &str {
        self.sdna.struct_name(self.blocks[block].sdna_index)
    }

    pub fn endian(&self) -> file::Endian {
        file::Endian(self.header.little_endian)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "shape-blend");
    }

    /// Build a minimal legacy-format file by hand: one `Thing` struct with a float array and a
    /// pointer, one DATA block it points at, and the DNA describing them.
    fn synthetic_file() -> Vec<u8> {
        let mut dna = Vec::new();
        dna.extend_from_slice(b"SDNA");
        dna.extend_from_slice(b"NAME");
        let names = ["co[3]", "*next", "count"];
        dna.extend_from_slice(&(names.len() as i32).to_le_bytes());
        for n in names {
            dna.extend_from_slice(n.as_bytes());
            dna.push(0);
        }
        while dna.len() % 4 != 0 {
            dna.push(0);
        }
        dna.extend_from_slice(b"TYPE");
        let types = ["float", "int", "Thing"];
        dna.extend_from_slice(&(types.len() as i32).to_le_bytes());
        for t in types {
            dna.extend_from_slice(t.as_bytes());
            dna.push(0);
        }
        while dna.len() % 4 != 0 {
            dna.push(0);
        }
        dna.extend_from_slice(b"TLEN");
        for len in [4i16, 4, 24] {
            dna.extend_from_slice(&len.to_le_bytes());
        }
        while dna.len() % 4 != 0 {
            dna.push(0);
        }
        dna.extend_from_slice(b"STRC");
        dna.extend_from_slice(&1i32.to_le_bytes());
        dna.extend_from_slice(&2i16.to_le_bytes()); // Thing
        dna.extend_from_slice(&3i16.to_le_bytes()); // 3 fields
        for (t, n) in [(0i16, 0i16), (2, 1), (1, 2)] {
            // float co[3]; Thing *next; int count
            dna.extend_from_slice(&t.to_le_bytes());
            dna.extend_from_slice(&n.to_le_bytes());
        }

        let mut thing = Vec::new();
        for f in [1.0f32, 2.0, 3.0] {
            thing.extend_from_slice(&f.to_le_bytes());
        }
        thing.extend_from_slice(&0x1000u64.to_le_bytes());
        thing.extend_from_slice(&7i32.to_le_bytes());

        let mut out = Vec::new();
        out.extend_from_slice(b"BLENDER-v405");
        let block = |out: &mut Vec<u8>, code: &[u8; 4], old: u64, sdna: i32, count: i32, data: &[u8]| {
            out.extend_from_slice(code);
            out.extend_from_slice(&(data.len() as i32).to_le_bytes());
            out.extend_from_slice(&old.to_le_bytes());
            out.extend_from_slice(&sdna.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(data);
        };
        block(&mut out, b"TH\0\0", 0x2000, 0, 1, &thing);
        block(&mut out, b"DATA", 0x1000, 0, 1, &[9u8; 24]);
        block(&mut out, b"DNA1", 0, 0, 1, &dna);
        block(&mut out, b"ENDB", 0, 0, 0, &[]);
        out
    }

    #[test]
    fn parses_a_synthetic_file() {
        let file = BlendFile::parse(&synthetic_file()).unwrap();
        assert_eq!(file.version(), 405);
        assert_eq!(file.header.pointer_size, 8);
        assert_eq!(file.blocks.len(), 3);
        assert_eq!(file.sdna.struct_index("Thing"), Some(0));
        assert_eq!(file.sdna.struct_size(0), 24);
        let (bi, _) = file.ids(b"TH").next().unwrap();
        let v = file.view(bi, 0).unwrap();
        assert_eq!(v.f32s("co").unwrap(), vec![1.0, 2.0, 3.0]);
        assert_eq!(v.i32("count"), Some(7));
        let next = v.pointer("next").unwrap();
        let target = file.resolve(next, bi).unwrap();
        assert_eq!(file.data(target), &[9u8; 24]);
        assert!(file.resolve(0, bi).is_none());
    }

    #[test]
    fn new_style_header() {
        let h = file::parse_header(b"BLENDER17-01v0501REND").unwrap();
        assert_eq!((h.version, h.pointer_size, h.format, h.len), (501, 8, 1, 17));
        let h = file::parse_header(b"BLENDER_V257").unwrap();
        assert_eq!((h.version, h.pointer_size, h.little_endian), (257, 4, false));
        assert!(file::parse_header(b"BLENDEX-v300").is_err());
    }

    /// Real files from a Blender checkout, when one is around (`SHAPE_BLEND_SAMPLES` = directory).
    #[test]
    fn real_samples_when_available() {
        let Some(dir) = std::env::var_os("SHAPE_BLEND_SAMPLES") else { return };
        let mut seen = 0;
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "blend") {
                let scene = import::import(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(scene.object_count() > 0, "{}", path.display());
                seen += 1;
            }
        }
        assert!(seen > 0);
    }
}
