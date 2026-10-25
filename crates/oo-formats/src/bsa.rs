//! BSA archives, versions 103 (Oblivion) and 104 (Fallout 3 / Skyrim LE).
//!
//! Layout reference: <https://en.uesp.net/wiki/Oblivion_Mod:BSA_File_Format>

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use flate2::read::ZlibDecoder;

use crate::{Error, Result};

const MAGIC: [u8; 4] = *b"BSA\0";

/// Archive flags from the header.
pub mod flags {
    pub const DIRECTORY_NAMES: u32 = 0x1;
    pub const FILE_NAMES: u32 = 0x2;
    pub const COMPRESSED: u32 = 0x4;
    /// Version 104 only: each file's data is prefixed with its full path.
    pub const EMBEDDED_NAMES: u32 = 0x100;
}

/// Set in a file record's size to invert the archive's default compression.
const SIZE_COMPRESSION_TOGGLE: u32 = 0x4000_0000;

#[derive(Debug, Clone)]
pub struct Entry {
    /// Lowercase, backslash-separated path, e.g. `meshes\clutter\apple01.nif`.
    pub path: String,
    pub offset: u32,
    /// Stored size in bytes (compressed size when compressed).
    pub size: u32,
    pub compressed: bool,
}

pub struct Archive {
    reader: BufReader<File>,
    version: u32,
    archive_flags: u32,
    entries: Vec<Entry>,
    by_path: HashMap<String, usize>,
}

impl Archive {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut r = BufReader::new(File::open(path)?);

        let magic = read_array::<4>(&mut r)?;
        if magic != MAGIC {
            return Err(Error::BadMagic {
                expected: "BSA",
                found: magic,
            });
        }
        let version = read_u32(&mut r)?;
        if version != 103 && version != 104 {
            return Err(Error::UnsupportedVersion {
                format: "BSA",
                version,
            });
        }
        let folder_offset = read_u32(&mut r)?;
        let archive_flags = read_u32(&mut r)?;
        let folder_count = read_u32(&mut r)?;
        let file_count = read_u32(&mut r)?;
        let _total_folder_name_len = read_u32(&mut r)?;
        let total_file_name_len = read_u32(&mut r)?;
        let _file_flags = read_u32(&mut r)?;

        r.seek(SeekFrom::Start(folder_offset.into()))?;
        let mut folder_sizes = Vec::with_capacity(folder_count as usize);
        for _ in 0..folder_count {
            let _hash = read_u64(&mut r)?;
            let count = read_u32(&mut r)?;
            let _offset = read_u32(&mut r)?;
            folder_sizes.push(count);
        }

        let default_compressed = archive_flags & flags::COMPRESSED != 0;
        let mut entries = Vec::with_capacity(file_count as usize);
        for count in folder_sizes {
            let folder = if archive_flags & flags::DIRECTORY_NAMES != 0 {
                read_bzstring(&mut r)?
            } else {
                String::new()
            };
            for _ in 0..count {
                let _hash = read_u64(&mut r)?;
                let raw_size = read_u32(&mut r)?;
                let offset = read_u32(&mut r)?;
                entries.push(Entry {
                    path: folder.clone(),
                    offset,
                    size: raw_size & !SIZE_COMPRESSION_TOGGLE,
                    compressed: default_compressed ^ (raw_size & SIZE_COMPRESSION_TOGGLE != 0),
                });
            }
        }
        if entries.len() != file_count as usize {
            return Err(Error::Corrupt(format!(
                "header says {file_count} files, folders list {}",
                entries.len()
            )));
        }

        if archive_flags & flags::FILE_NAMES != 0 {
            let mut names = vec![0u8; total_file_name_len as usize];
            r.read_exact(&mut names)?;
            let mut names = names.split(|&b| b == 0);
            for entry in &mut entries {
                let name = names
                    .next()
                    .ok_or_else(|| Error::Corrupt("file name block too short".into()))?;
                let name = latin1(name).to_ascii_lowercase();
                entry.path = if entry.path.is_empty() {
                    name
                } else {
                    format!("{}\\{}", entry.path, name)
                };
            }
        }

        let by_path = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path.clone(), i))
            .collect();

        Ok(Self {
            reader: r,
            version,
            archive_flags,
            entries,
            by_path,
        })
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Looks a file up by path. Case and slash direction don't matter.
    pub fn find(&self, path: &str) -> Option<&Entry> {
        self.by_path
            .get(&normalize(path))
            .map(|&i| &self.entries[i])
    }

    /// Reads and, if needed, decompresses a file.
    pub fn read(&mut self, path: &str) -> Result<Vec<u8>> {
        let entry = self
            .find(path)
            .ok_or_else(|| Error::NotFound(path.to_owned()))?
            .clone();
        self.read_entry(&entry)
    }

    pub fn read_entry(&mut self, entry: &Entry) -> Result<Vec<u8>> {
        let r = &mut self.reader;
        r.seek(SeekFrom::Start(entry.offset.into()))?;
        let mut remaining = entry.size as usize;

        if self.version == 104 && self.archive_flags & flags::EMBEDDED_NAMES != 0 {
            let len = read_array::<1>(r)?[0] as usize;
            r.seek_relative(len as i64)?;
            remaining = remaining
                .checked_sub(len + 1)
                .ok_or_else(|| Error::Corrupt(format!("{}: embedded name too long", entry.path)))?;
        }

        if entry.compressed {
            let original_size = read_u32(r)? as usize;
            remaining = remaining
                .checked_sub(4)
                .ok_or_else(|| Error::Corrupt(format!("{}: compressed size < 4", entry.path)))?;
            let mut out = Vec::with_capacity(original_size);
            ZlibDecoder::new(r.take(remaining as u64)).read_to_end(&mut out)?;
            if out.len() != original_size {
                return Err(Error::Corrupt(format!(
                    "{}: expected {original_size} bytes after inflating, got {}",
                    entry.path,
                    out.len()
                )));
            }
            Ok(out)
        } else {
            let mut out = vec![0u8; remaining];
            r.read_exact(&mut out)?;
            Ok(out)
        }
    }
}

fn normalize(path: &str) -> String {
    path.replace('/', "\\").to_ascii_lowercase()
}

/// Oblivion strings are Windows-1252; Latin-1 is close enough for paths.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// A length-prefixed, null-terminated string; the length counts the null.
fn read_bzstring(r: &mut impl Read) -> Result<String> {
    let len = read_array::<1>(r)?[0] as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    if buf.last() == Some(&0) {
        buf.pop();
    }
    Ok(latin1(&buf).to_ascii_lowercase())
}

fn read_array<const N: usize>(r: &mut impl Read) -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_u32(r: &mut impl Read) -> Result<u32> {
    Ok(u32::from_le_bytes(read_array(r)?))
}

fn read_u64(r: &mut impl Read) -> Result<u64> {
    Ok(u64::from_le_bytes(read_array(r)?))
}
