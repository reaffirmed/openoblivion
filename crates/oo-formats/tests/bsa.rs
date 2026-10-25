//! Builds small v103 archives in memory and reads them back.

use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use oo_formats::bsa::{Archive, flags};

struct Folder<'a> {
    name: &'a str,
    files: Vec<(&'a str, Vec<u8>, bool)>,
}

/// Writes a v103 archive. `compress_default` sets the archive flag; each file's
/// own bool says whether it is actually stored compressed.
fn build(folders: &[Folder], compress_default: bool) -> Vec<u8> {
    let archive_flags = flags::DIRECTORY_NAMES
        | flags::FILE_NAMES
        | if compress_default {
            flags::COMPRESSED
        } else {
            0
        };
    let file_count: usize = folders.iter().map(|f| f.files.len()).sum();
    let folder_names_len: usize = folders.iter().map(|f| f.name.len() + 1).sum();
    let file_names_len: usize = folders
        .iter()
        .flat_map(|f| &f.files)
        .map(|(n, ..)| n.len() + 1)
        .sum();

    let header_len = 36;
    let folder_records_len = 16 * folders.len();
    let file_records_len = folders.len() + folder_names_len + 16 * file_count;
    let mut data_offset = header_len + folder_records_len + file_records_len + file_names_len;

    let mut out = Vec::new();
    out.extend_from_slice(b"BSA\0");
    for v in [
        103,
        header_len as u32,
        archive_flags,
        folders.len() as u32,
        file_count as u32,
        folder_names_len as u32,
        file_names_len as u32,
        0,
    ] {
        out.extend_from_slice(&u32::to_le_bytes(v));
    }

    for f in folders {
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&(f.files.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
    }

    let mut blobs = Vec::new();
    for f in folders {
        out.push((f.name.len() + 1) as u8);
        out.extend_from_slice(f.name.as_bytes());
        out.push(0);
        for (_, contents, compressed) in &f.files {
            let blob = if *compressed {
                let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
                enc.write_all(contents).unwrap();
                let mut b = (contents.len() as u32).to_le_bytes().to_vec();
                b.extend(enc.finish().unwrap());
                b
            } else {
                contents.clone()
            };
            let mut size = blob.len() as u32;
            if *compressed != compress_default {
                size |= 0x4000_0000;
            }
            out.extend_from_slice(&0u64.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&(data_offset as u32).to_le_bytes());
            data_offset += blob.len();
            blobs.push(blob);
        }
    }
    for f in folders {
        for (name, ..) in &f.files {
            out.extend_from_slice(name.as_bytes());
            out.push(0);
        }
    }
    for b in blobs {
        out.extend(b);
    }
    out
}

fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("oo-formats-test-{}-{name}", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn reads_plain_and_compressed_files() {
    let apple = b"NIF apple mesh".repeat(20);
    let readme = b"hello".to_vec();
    let bytes = build(
        &[
            Folder {
                name: "meshes\\clutter",
                files: vec![("Apple01.nif", apple.clone(), true)],
            },
            Folder {
                name: "docs",
                files: vec![("readme.txt", readme.clone(), false)],
            },
        ],
        false,
    );
    let path = write_temp("plain.bsa", &bytes);
    let mut bsa = Archive::open(&path).unwrap();

    assert_eq!(bsa.version(), 103);
    let paths: Vec<_> = bsa.entries().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["meshes\\clutter\\apple01.nif", "docs\\readme.txt"]);
    assert_eq!(bsa.read("Meshes/Clutter/Apple01.NIF").unwrap(), apple);
    assert_eq!(bsa.read("docs\\readme.txt").unwrap(), readme);
    assert!(bsa.read("nope.dds").is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn compression_toggle_inverts_archive_default() {
    let tex = b"DDS texture".repeat(50);
    let raw = b"uncompressed in a compressed archive".to_vec();
    let bytes = build(
        &[Folder {
            name: "textures",
            files: vec![("a.dds", tex.clone(), true), ("b.txt", raw.clone(), false)],
        }],
        true,
    );
    let path = write_temp("compressed.bsa", &bytes);
    let mut bsa = Archive::open(&path).unwrap();

    assert!(bsa.find("textures\\a.dds").unwrap().compressed);
    assert!(!bsa.find("textures\\b.txt").unwrap().compressed);
    assert_eq!(bsa.read("textures\\a.dds").unwrap(), tex);
    assert_eq!(bsa.read("textures\\b.txt").unwrap(), raw);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn rejects_non_bsa() {
    let path = write_temp("bad.bsa", b"not an archive at all, really not");
    assert!(Archive::open(&path).is_err());
    std::fs::remove_file(path).unwrap();
}
