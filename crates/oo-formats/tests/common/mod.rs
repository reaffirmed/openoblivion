//! Builders for synthetic plugin bytes, shared by the integration tests.
#![allow(dead_code)]

use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use oo_formats::esm::flags;

pub fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    if data.len() > u16::MAX as usize {
        out = b"XXXX".to_vec();
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u16.to_le_bytes());
    } else {
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    }
    out.extend_from_slice(data);
    out
}

pub fn zstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

pub fn record(tag: &[u8; 4], flags: u32, form_id: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    let mut body: Vec<u8> = fields.concat();
    if flags & flags::COMPRESSED != 0 {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&body).unwrap();
        let mut packed = (body.len() as u32).to_le_bytes().to_vec();
        packed.extend(enc.finish().unwrap());
        body = packed;
    }
    let mut out = tag.to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&form_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend(body);
    out
}

pub fn group(label: &[u8; 4], kind: i32, children: &[Vec<u8>]) -> Vec<u8> {
    let body = children.concat();
    let mut out = b"GRUP".to_vec();
    out.extend_from_slice(&((20 + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(label);
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend(body);
    out
}
