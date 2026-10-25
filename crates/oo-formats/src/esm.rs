//! ESM/ESP plugins in the TES4 (Oblivion) layout.
//!
//! A plugin is a `TES4` header record followed by top-level `GRUP`s, each of
//! which holds records or nested groups. Records hold typed subrecords
//! ("fields"). Oblivion's record and group headers are 20 bytes; later games
//! use 24, so this module only reads Oblivion plugins.
//!
//! Layout reference: <https://en.uesp.net/wiki/Oblivion_Mod:Mod_File_Format>

use std::borrow::Cow;
use std::fmt;
use std::io::Read;
use std::path::Path;

use flate2::read::ZlibDecoder;

use crate::{Error, Result};

const HEADER_LEN: usize = 20;

/// A four-character code such as `CELL` or `EDID`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tag(pub [u8; 4]);

impl Tag {
    pub const TES4: Tag = Tag(*b"TES4");
    pub const GRUP: Tag = Tag(*b"GRUP");
    pub const XXXX: Tag = Tag(*b"XXXX");
    pub const EDID: Tag = Tag(*b"EDID");
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in &self.0 {
            let c = if b.is_ascii_graphic() { b as char } else { '?' };
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tag({self})")
    }
}

/// Record header flags.
pub mod flags {
    pub const MASTER: u32 = 0x0000_0001;
    pub const DELETED: u32 = 0x0000_0020;
    pub const COMPRESSED: u32 = 0x0004_0000;
}

/// The contents of the `TES4` header record.
#[derive(Debug, Clone, Default)]
pub struct Header {
    pub version: f32,
    pub num_records: i32,
    pub next_object_id: u32,
    pub author: String,
    pub description: String,
    pub masters: Vec<String>,
    pub is_master: bool,
}

pub struct Plugin {
    bytes: Vec<u8>,
    header: Header,
    body_offset: usize,
}

impl Plugin {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let mut nodes = Nodes::new(&bytes);
        let first = match nodes.next() {
            Some(Ok(Node::Record(r))) if r.tag == Tag::TES4 => r,
            Some(Ok(Node::Record(r))) => {
                return Err(Error::BadMagic {
                    expected: "TES4 plugin",
                    found: r.tag.0,
                });
            }
            Some(Ok(Node::Group(_))) => {
                return Err(Error::BadMagic {
                    expected: "TES4 plugin",
                    found: Tag::GRUP.0,
                });
            }
            Some(Err(e)) => return Err(e),
            None => return Err(Error::Corrupt("empty plugin".into())),
        };

        let mut header = Header {
            is_master: first.flags & flags::MASTER != 0,
            ..Default::default()
        };
        let fields = first.fields()?;
        for field in fields.iter() {
            let field = field?;
            match &field.tag.0 {
                b"HEDR" => {
                    let d = field.data;
                    if d.len() < 12 {
                        return Err(Error::Corrupt("HEDR shorter than 12 bytes".into()));
                    }
                    header.version = f32::from_le_bytes(d[0..4].try_into().unwrap());
                    header.num_records = i32::from_le_bytes(d[4..8].try_into().unwrap());
                    header.next_object_id = u32::from_le_bytes(d[8..12].try_into().unwrap());
                }
                b"CNAM" => header.author = field.zstring(),
                b"SNAM" => header.description = field.zstring(),
                b"MAST" => header.masters.push(field.zstring()),
                _ => {}
            }
        }

        let body_offset = nodes.offset;
        Ok(Self {
            bytes,
            header,
            body_offset,
        })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The top-level groups (and any stray top-level records) after the header.
    pub fn top_level(&self) -> Nodes<'_> {
        Nodes::new(&self.bytes[self.body_offset..])
    }

    /// Calls `f` for every record in the plugin, depth first, excluding the header.
    pub fn for_each_record(&self, mut f: impl FnMut(&Record<'_>)) -> Result<()> {
        fn walk<'a>(nodes: Nodes<'a>, f: &mut impl FnMut(&Record<'a>)) -> Result<()> {
            for node in nodes {
                match node? {
                    Node::Record(r) => f(&r),
                    Node::Group(g) => walk(g.children(), f)?,
                }
            }
            Ok(())
        }
        walk(self.top_level(), &mut f)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Node<'a> {
    Group(Group<'a>),
    Record(Record<'a>),
}

/// What a group's label means. See the UESP page for the full table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    /// Label is a record type, e.g. all `WEAP` records.
    Top,
    WorldChildren,
    InteriorCellBlock,
    InteriorCellSubBlock,
    ExteriorCellBlock,
    ExteriorCellSubBlock,
    CellChildren,
    TopicChildren,
    CellPersistentChildren,
    CellTemporaryChildren,
    CellVisibleDistantChildren,
    Unknown(i32),
}

impl From<i32> for GroupKind {
    fn from(v: i32) -> Self {
        match v {
            0 => Self::Top,
            1 => Self::WorldChildren,
            2 => Self::InteriorCellBlock,
            3 => Self::InteriorCellSubBlock,
            4 => Self::ExteriorCellBlock,
            5 => Self::ExteriorCellSubBlock,
            6 => Self::CellChildren,
            7 => Self::TopicChildren,
            8 => Self::CellPersistentChildren,
            9 => Self::CellTemporaryChildren,
            10 => Self::CellVisibleDistantChildren,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Group<'a> {
    pub label: [u8; 4],
    pub kind: GroupKind,
    pub stamp: u32,
    body: &'a [u8],
}

impl<'a> Group<'a> {
    /// For top-level groups, the record type the group contains.
    pub fn record_type(&self) -> Option<Tag> {
        (self.kind == GroupKind::Top).then_some(Tag(self.label))
    }

    pub fn children(&self) -> Nodes<'a> {
        Nodes::new(self.body)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    pub tag: Tag,
    pub flags: u32,
    pub form_id: u32,
    pub vc_info: u32,
    body: &'a [u8],
}

impl<'a> Record<'a> {
    pub fn is_compressed(&self) -> bool {
        self.flags & flags::COMPRESSED != 0
    }

    pub fn is_deleted(&self) -> bool {
        self.flags & flags::DELETED != 0
    }

    /// The record's subrecords, inflating the body first if it's compressed.
    pub fn fields(&self) -> Result<Fields<'a>> {
        if !self.is_compressed() {
            return Ok(Fields {
                data: Cow::Borrowed(self.body),
            });
        }
        let corrupt =
            |msg: &str| Error::Corrupt(format!("{} {:08X}: {msg}", self.tag, self.form_id));
        if self.body.len() < 4 {
            return Err(corrupt("compressed body shorter than 4 bytes"));
        }
        let size = u32::from_le_bytes(self.body[..4].try_into().unwrap()) as usize;
        let mut out = Vec::with_capacity(size);
        ZlibDecoder::new(&self.body[4..]).read_to_end(&mut out)?;
        if out.len() != size {
            return Err(corrupt("inflated size doesn't match header"));
        }
        Ok(Fields {
            data: Cow::Owned(out),
        })
    }

    /// The editor ID (`EDID`), if the record has one.
    pub fn editor_id(&self) -> Result<Option<String>> {
        let fields = self.fields()?;
        for field in fields.iter() {
            let field = field?;
            if field.tag == Tag::EDID {
                return Ok(Some(field.zstring()));
            }
        }
        Ok(None)
    }
}

/// Iterates over the groups and records in a byte range.
pub struct Nodes<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Nodes<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_node(&mut self) -> Result<Node<'a>> {
        let rest = &self.bytes[self.offset..];
        if rest.len() < HEADER_LEN {
            return Err(Error::Corrupt(format!(
                "truncated header at offset {}",
                self.offset
            )));
        }
        let tag = Tag(rest[0..4].try_into().unwrap());
        let size = u32::from_le_bytes(rest[4..8].try_into().unwrap()) as usize;
        let a = rest[8..12].try_into().unwrap();
        let b = u32::from_le_bytes(rest[12..16].try_into().unwrap());
        let c = u32::from_le_bytes(rest[16..20].try_into().unwrap());

        // A group's size includes its header; a record's doesn't.
        let total = if tag == Tag::GRUP {
            size
        } else {
            HEADER_LEN + size
        };
        if total < HEADER_LEN || total > rest.len() {
            return Err(Error::Corrupt(format!(
                "{tag} at offset {} claims {total} bytes, {} available",
                self.offset,
                rest.len()
            )));
        }
        let body = &rest[HEADER_LEN..total];
        self.offset += total;

        Ok(if tag == Tag::GRUP {
            Node::Group(Group {
                label: a,
                kind: GroupKind::from(b as i32),
                stamp: c,
                body,
            })
        } else {
            Node::Record(Record {
                tag,
                flags: u32::from_le_bytes(a),
                form_id: b,
                vc_info: c,
                body,
            })
        })
    }
}

impl<'a> Iterator for Nodes<'a> {
    type Item = Result<Node<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.bytes.len() {
            return None;
        }
        let node = self.read_node();
        if node.is_err() {
            // Stop after the first error rather than reading garbage.
            self.offset = self.bytes.len();
        }
        Some(node)
    }
}

/// A record's (possibly inflated) subrecord data.
pub struct Fields<'a> {
    data: Cow<'a, [u8]>,
}

impl Fields<'_> {
    pub fn iter(&self) -> FieldIter<'_> {
        FieldIter {
            bytes: &self.data,
            offset: 0,
            next_size: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Field<'a> {
    pub tag: Tag,
    pub data: &'a [u8],
}

impl Field<'_> {
    /// The data as a null-terminated Windows-1252 string (decoded as Latin-1).
    pub fn zstring(&self) -> String {
        let end = self
            .data
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.data.len());
        self.data[..end].iter().map(|&b| b as char).collect()
    }
}

pub struct FieldIter<'a> {
    bytes: &'a [u8],
    offset: usize,
    /// Set by an `XXXX` field: the real size of the next field, which may exceed u16.
    next_size: Option<usize>,
}

impl<'a> Iterator for FieldIter<'a> {
    type Item = Result<Field<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let rest = &self.bytes[self.offset..];
            if rest.is_empty() {
                return None;
            }
            if rest.len() < 6 {
                self.offset = self.bytes.len();
                return Some(Err(Error::Corrupt("truncated subrecord header".into())));
            }
            let tag = Tag(rest[0..4].try_into().unwrap());
            let short_size = u16::from_le_bytes(rest[4..6].try_into().unwrap()) as usize;
            let size = self.next_size.take().unwrap_or(short_size);
            if 6 + size > rest.len() {
                self.offset = self.bytes.len();
                return Some(Err(Error::Corrupt(format!(
                    "subrecord {tag} overruns its record"
                ))));
            }
            let data = &rest[6..6 + size];
            self.offset += 6 + size;

            if tag == Tag::XXXX {
                if data.len() != 4 {
                    self.offset = self.bytes.len();
                    return Some(Err(Error::Corrupt("XXXX subrecord isn't 4 bytes".into())));
                }
                self.next_size = Some(u32::from_le_bytes(data.try_into().unwrap()) as usize);
                continue;
            }
            return Some(Ok(Field { tag, data }));
        }
    }
}
