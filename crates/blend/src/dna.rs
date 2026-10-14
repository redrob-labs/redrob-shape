//! SDNA: the struct catalogue every .blend carries in its `DNA1` block, and typed field access.

use crate::file::Endian;
use crate::{BlendError, Result};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Field {
    /// Raw name as written, e.g. `*next`, `co[3]`, `(*func)()`.
    pub name: String,
    pub type_index: usize,
}

#[derive(Clone, Debug)]
pub struct Struct {
    pub type_index: usize,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug)]
pub struct Sdna {
    pub names: Vec<String>,
    pub types: Vec<String>,
    pub sizes: Vec<u16>,
    pub structs: Vec<Struct>,
    pub struct_by_type: HashMap<usize, usize>,
    pub pointer_size: usize,
    pub endian: Endian,
}

/// Where a field lives inside a struct and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldInfo {
    pub offset: usize,
    pub size: usize,
    pub type_name: String,
    /// Product of the array dimensions, 1 for scalars.
    pub array_len: usize,
    pub is_pointer: bool,
}

impl Sdna {
    pub fn parse(bytes: &[u8], pointer_size: usize, endian: Endian) -> Result<Self> {
        let e = endian;
        let mut p = 0usize;
        let tag = |p: usize, want: &[u8]| -> Result<()> {
            if bytes.len() < p + 4 || &bytes[p..p + 4] != want {
                return Err(BlendError::Dna(format!("expected {:?} at {p}", String::from_utf8_lossy(want))));
            }
            Ok(())
        };
        tag(p, b"SDNA")?;
        p += 4;
        tag(p, b"NAME")?;
        p += 4;
        let n = e.i32(&bytes[p..]) as usize;
        p += 4;
        let mut names = Vec::with_capacity(n);
        for _ in 0..n {
            let s = cstr(bytes, p)?;
            p += s.len() + 1;
            names.push(s);
        }
        p = pad4(p);
        tag(p, b"TYPE")?;
        p += 4;
        let n = e.i32(&bytes[p..]) as usize;
        p += 4;
        let mut types = Vec::with_capacity(n);
        for _ in 0..n {
            let s = cstr(bytes, p)?;
            p += s.len() + 1;
            types.push(s);
        }
        p = pad4(p);
        tag(p, b"TLEN")?;
        p += 4;
        let mut sizes = Vec::with_capacity(types.len());
        for _ in 0..types.len() {
            sizes.push(e.i16(&bytes[p..]) as u16);
            p += 2;
        }
        p = pad4(p);
        tag(p, b"STRC")?;
        p += 4;
        let n = e.i32(&bytes[p..]) as usize;
        p += 4;
        let mut structs = Vec::with_capacity(n);
        let mut struct_by_type = HashMap::new();
        for si in 0..n {
            let type_index = e.i16(&bytes[p..]) as usize;
            let field_count = e.i16(&bytes[p + 2..]) as usize;
            p += 4;
            let mut fields = Vec::with_capacity(field_count);
            for _ in 0..field_count {
                let t = e.i16(&bytes[p..]) as usize;
                let nm = e.i16(&bytes[p + 2..]) as usize;
                p += 4;
                let name = names.get(nm).ok_or_else(|| BlendError::Dna("field name index".into()))?.clone();
                if t >= types.len() {
                    return Err(BlendError::Dna("field type index".into()));
                }
                fields.push(Field { name, type_index: t });
            }
            struct_by_type.insert(type_index, si);
            structs.push(Struct { type_index, fields });
        }
        Ok(Self { names, types, sizes, structs, struct_by_type, pointer_size, endian })
    }

    pub fn struct_index(&self, name: &str) -> Option<usize> {
        let t = self.types.iter().position(|t| t == name)?;
        self.struct_by_type.get(&t).copied()
    }

    pub fn struct_name(&self, index: usize) -> &str {
        &self.types[self.structs[index].type_index]
    }

    pub fn struct_size(&self, index: usize) -> usize {
        self.sizes[self.structs[index].type_index] as usize
    }

    /// Size one field occupies.
    fn field_size(&self, f: &Field) -> usize {
        let array_len = array_len(&f.name);
        if f.name.starts_with('*') || f.name.starts_with("(*") {
            self.pointer_size * array_len
        } else {
            self.sizes[f.type_index] as usize * array_len
        }
    }

    /// Find a field of a struct by its bare name (without `*` or `[n]`).
    pub fn field(&self, struct_index: usize, name: &str) -> Option<FieldInfo> {
        let mut offset = 0;
        for f in &self.structs[struct_index].fields {
            let size = self.field_size(f);
            if bare_name(&f.name) == name {
                return Some(FieldInfo {
                    offset,
                    size,
                    type_name: self.types[f.type_index].clone(),
                    array_len: array_len(&f.name),
                    is_pointer: f.name.starts_with('*') || f.name.starts_with("(*"),
                });
            }
            offset += size;
        }
        None
    }
}

/// A struct instance laid over a byte slice.
#[derive(Clone, Copy)]
pub struct View<'a> {
    pub sdna: &'a Sdna,
    pub struct_index: usize,
    pub bytes: &'a [u8],
}

impl<'a> View<'a> {
    fn slot(&self, name: &str) -> Option<(FieldInfo, &'a [u8])> {
        let f = self.sdna.field(self.struct_index, name)?;
        let end = f.offset + f.size;
        (end <= self.bytes.len()).then(|| {
            let b = &self.bytes[f.offset..end];
            (f, b)
        })
    }

    pub fn has(&self, name: &str) -> bool {
        self.sdna.field(self.struct_index, name).is_some()
    }

    pub fn f32(&self, name: &str) -> Option<f32> {
        let (_, b) = self.slot(name)?;
        Some(self.sdna.endian.f32(b))
    }
    pub fn f32s(&self, name: &str) -> Option<Vec<f32>> {
        let (f, b) = self.slot(name)?;
        Some((0..f.array_len).map(|i| self.sdna.endian.f32(&b[i * 4..])).collect())
    }
    pub fn i32(&self, name: &str) -> Option<i32> {
        let (f, b) = self.slot(name)?;
        Some(match f.type_name.as_str() {
            "short" | "ushort" => self.sdna.endian.i16(b) as i32,
            "char" | "uchar" | "int8_t" => b[0] as i8 as i32,
            "int64_t" | "uint64_t" => self.sdna.endian.i64(b) as i32,
            _ => self.sdna.endian.i32(b),
        })
    }
    pub fn pointer(&self, name: &str) -> Option<u64> {
        let (f, b) = self.slot(name)?;
        f.is_pointer.then(|| self.sdna.endian.pointer(b, self.sdna.pointer_size))
    }
    /// Fixed `char name[n]` field as a string up to the first NUL.
    pub fn string(&self, name: &str) -> Option<String> {
        let (_, b) = self.slot(name)?;
        Some(String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned())
    }
    /// Embedded struct field (not a pointer) as a view.
    pub fn sub(&self, name: &str) -> Option<View<'a>> {
        let (f, b) = self.slot(name)?;
        if f.is_pointer {
            return None;
        }
        let t = self.sdna.types.iter().position(|t| *t == f.type_name)?;
        let struct_index = *self.sdna.struct_by_type.get(&t)?;
        Some(View { sdna: self.sdna, struct_index, bytes: b })
    }
}

fn cstr(bytes: &[u8], p: usize) -> Result<String> {
    let end = bytes[p..].iter().position(|&c| c == 0).ok_or_else(|| BlendError::Dna("unterminated string".into()))?;
    Ok(String::from_utf8_lossy(&bytes[p..p + end]).into_owned())
}

fn pad4(p: usize) -> usize {
    (p + 3) & !3
}

/// `co[3]` -> 3, `mat[4][4]` -> 16, `*next` -> 1.
pub fn array_len(name: &str) -> usize {
    let mut len = 1;
    let mut rest = name;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else { break };
        if let Ok(n) = rest[open + 1..open + close].parse::<usize>() {
            len *= n;
        }
        rest = &rest[open + close + 1..];
    }
    len
}

/// `*next` -> `next`, `co[3]` -> `co`, `(*func)()` -> `func`.
pub fn bare_name(name: &str) -> &str {
    let s = name.trim_start_matches('(').trim_start_matches('*');
    let end = s.find(['[', ')']).unwrap_or(s.len());
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_arrays() {
        assert_eq!(array_len("co[3]"), 3);
        assert_eq!(array_len("mat[4][4]"), 16);
        assert_eq!(array_len("*next"), 1);
        assert_eq!(bare_name("*next"), "next");
        assert_eq!(bare_name("co[3]"), "co");
        assert_eq!(bare_name("(*func)()"), "func");
        assert_eq!(bare_name("**mat"), "mat");
    }
}
