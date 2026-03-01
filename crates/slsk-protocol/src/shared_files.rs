//! Shared file list encoding / decoding.
//!
//! The wire format is zlib-compressed. This module handles the
//! inner (uncompressed) layout used by [`SharedFileListResponse`] and
//! [`FileSearchResponse`].

use std::io::Cursor;

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use std::io::{Read, Write};

use crate::{
    codec::*,
    error::{Error, Result},
    types::{FileAttribute, FileAttributeType, SharedFile},
};

// ── Shared file entry helpers ─────────────────────────────────────────────────

pub fn decode_shared_file(cur: &mut Cursor<&[u8]>) -> Result<SharedFile> {
    let code = read_u8(cur)?;
    let filename = read_string(cur)?;
    let size = read_u64_le(cur)?;
    let extension = read_string(cur)?;
    let n_attr = read_u32_le(cur)? as usize;
    let mut attributes = Vec::with_capacity(n_attr);
    for _ in 0..n_attr {
        let attr_code = read_u32_le(cur)?;
        let attr_value = read_u32_le(cur)?;
        if let Ok(t) = FileAttributeType::try_from(attr_code) {
            attributes.push(FileAttribute {
                code: t,
                value: attr_value,
            });
        }
    }
    Ok(SharedFile {
        code,
        filename,
        size,
        extension,
        attributes,
    })
}

pub fn encode_shared_file(f: &SharedFile, out: &mut Vec<u8>) {
    write_u8(out, f.code).unwrap();
    write_string(out, &f.filename).unwrap();
    write_u64_le(out, f.size).unwrap();
    write_string(out, &f.extension).unwrap();
    write_u32_le(out, f.attributes.len() as u32).unwrap();
    for a in &f.attributes {
        write_u32_le(out, a.code as u32).unwrap();
        write_u32_le(out, a.value).unwrap();
    }
}

// ── Directory entry ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SharedDirectory {
    pub name: String,
    pub files: Vec<SharedFile>,
}

pub fn decode_directory(cur: &mut Cursor<&[u8]>) -> Result<SharedDirectory> {
    let name = read_string(cur)?;
    let n = read_u32_le(cur)? as usize;
    let mut files = Vec::with_capacity(n);
    for _ in 0..n {
        files.push(decode_shared_file(cur)?);
    }
    Ok(SharedDirectory { name, files })
}

pub fn encode_directory(dir: &SharedDirectory, out: &mut Vec<u8>) {
    write_string(out, &dir.name).unwrap();
    write_u32_le(out, dir.files.len() as u32).unwrap();
    for f in &dir.files {
        encode_shared_file(f, out);
    }
}

// ── SharedFileListResponse ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SharedFileListResponse {
    pub directories: Vec<SharedDirectory>,
    pub unknown: u32,
    pub private_directories: Vec<SharedDirectory>,
}

impl SharedFileListResponse {
    pub fn decode_compressed(data: &[u8]) -> Result<Self> {
        let mut decoder = ZlibDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| Error::Zlib(e.to_string()))?;

        let slice = decompressed.as_slice();
        let mut cur = Cursor::new(slice);

        let n_dirs = read_u32_le(&mut cur)? as usize;
        let mut directories = Vec::with_capacity(n_dirs);
        for _ in 0..n_dirs {
            directories.push(decode_directory(&mut cur)?);
        }
        let unknown = read_u32_le(&mut cur).unwrap_or(0);
        let n_priv = read_u32_le(&mut cur).unwrap_or(0) as usize;
        let mut private_directories = Vec::with_capacity(n_priv);
        for _ in 0..n_priv {
            if let Ok(d) = decode_directory(&mut cur) {
                private_directories.push(d);
            }
        }
        Ok(Self {
            directories,
            unknown,
            private_directories,
        })
    }

    pub fn encode_compressed(&self) -> Result<Vec<u8>> {
        let mut raw = Vec::new();
        write_u32_le(&mut raw, self.directories.len() as u32).unwrap();
        for d in &self.directories {
            encode_directory(d, &mut raw);
        }
        write_u32_le(&mut raw, self.unknown).unwrap();
        write_u32_le(&mut raw, self.private_directories.len() as u32).unwrap();
        for d in &self.private_directories {
            encode_directory(d, &mut raw);
        }

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&raw)
            .map_err(|e| Error::Zlib(e.to_string()))?;
        encoder.finish().map_err(|e| Error::Zlib(e.to_string()))
    }
}

// ── FolderContentsResponse ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FolderContentsResponse {
    pub token: u32,
    pub folder: String,
    pub directories: Vec<SharedDirectory>,
}

impl FolderContentsResponse {
    pub fn decode_compressed(data: &[u8]) -> Result<Self> {
        let mut decoder = ZlibDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| Error::Zlib(e.to_string()))?;

        let slice = decompressed.as_slice();
        let mut cur = Cursor::new(slice);
        let token = read_u32_le(&mut cur)?;
        let folder = read_string(&mut cur)?;
        let n = read_u32_le(&mut cur)? as usize;
        let mut directories = Vec::with_capacity(n);
        for _ in 0..n {
            directories.push(decode_directory(&mut cur)?);
        }
        Ok(Self {
            token,
            folder,
            directories,
        })
    }

    pub fn encode_compressed(&self) -> Result<Vec<u8>> {
        let mut raw = Vec::new();
        write_u32_le(&mut raw, self.token).unwrap();
        write_string(&mut raw, &self.folder).unwrap();
        write_u32_le(&mut raw, self.directories.len() as u32).unwrap();
        for d in &self.directories {
            encode_directory(d, &mut raw);
        }

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&raw)
            .map_err(|e| Error::Zlib(e.to_string()))?;
        encoder.finish().map_err(|e| Error::Zlib(e.to_string()))
    }
}
