//! Low-level encoding/decoding primitives for the Soulseek wire format.
//!
//! All integers are little-endian. Strings are length-prefixed with a uint32.

use crate::error::{Error, Result};
use std::io::{Read, Write};

// ── Read helpers ────────────────────────────────────────────────────────────

pub fn read_u8(r: &mut impl Read) -> Result<u8> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0])
}

pub fn read_u16_le(r: &mut impl Read) -> Result<u16> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

pub fn read_u32_le(r: &mut impl Read) -> Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

pub fn read_i32_le(r: &mut impl Read) -> Result<i32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(i32::from_le_bytes(buf))
}

pub fn read_u64_le(r: &mut impl Read) -> Result<u64> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

pub fn read_bool(r: &mut impl Read) -> Result<bool> {
    Ok(read_u8(r)? != 0)
}

pub fn read_string(r: &mut impl Read) -> Result<String> {
    let len = read_u32_le(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| Error::Encoding(e.to_string()))
}

pub fn read_bytes(r: &mut impl Read) -> Result<Vec<u8>> {
    let len = read_u32_le(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn read_raw_bytes(r: &mut impl Read, len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

// ── Write helpers ────────────────────────────────────────────────────────────

pub fn write_u8(w: &mut impl Write, v: u8) -> Result<()> {
    w.write_all(&[v])?;
    Ok(())
}

pub fn write_u16_le(w: &mut impl Write, v: u16) -> Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

pub fn write_u32_le(w: &mut impl Write, v: u32) -> Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

pub fn write_i32_le(w: &mut impl Write, v: i32) -> Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

pub fn write_u64_le(w: &mut impl Write, v: u64) -> Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

pub fn write_bool(w: &mut impl Write, v: bool) -> Result<()> {
    write_u8(w, v as u8)
}

pub fn write_string(w: &mut impl Write, s: &str) -> Result<()> {
    let bytes = s.as_bytes();
    write_u32_le(w, bytes.len() as u32)?;
    w.write_all(bytes)?;
    Ok(())
}

pub fn write_bytes(w: &mut impl Write, data: &[u8]) -> Result<()> {
    write_u32_le(w, data.len() as u32)?;
    w.write_all(data)?;
    Ok(())
}

// ── Framing ──────────────────────────────────────────────────────────────────

/// Wrap an already-encoded payload into the standard `[length][code][body]` frame.
/// Used for server and peer (P) messages (uint32 code).
pub fn frame_message_u32(code: u32, body: &[u8]) -> Vec<u8> {
    // length field = 4 bytes code + body length
    let total = 4 + body.len();
    let mut out = Vec::with_capacity(4 + total);
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&code.to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// Wrap a payload for peer-init and distributed messages (uint8 code).
pub fn frame_message_u8(code: u8, body: &[u8]) -> Vec<u8> {
    let total = 1 + body.len();
    let mut out = Vec::with_capacity(4 + total);
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.push(code);
    out.extend_from_slice(body);
    out
}

/// Read one framed message from a stream: returns `(code_byte, body)`.
/// Used for peer-init and distributed connections.
pub fn read_frame_u8(r: &mut impl Read) -> Result<(u8, Vec<u8>)> {
    let total = read_u32_le(r)? as usize;
    if total == 0 {
        return Err(Error::Protocol("empty frame".into()));
    }
    let mut buf = vec![0u8; total];
    r.read_exact(&mut buf)?;
    let code = buf[0];
    Ok((code, buf[1..].to_vec()))
}

/// Read one framed message from a stream: returns `(code_u32, body)`.
/// Used for server and peer (P) connections.
pub fn read_frame_u32(r: &mut impl Read) -> Result<(u32, Vec<u8>)> {
    let total = read_u32_le(r)? as usize;
    if total < 4 {
        return Err(Error::Protocol(format!("frame too short: {}", total)));
    }
    let mut buf = vec![0u8; total];
    r.read_exact(&mut buf)?;
    let code = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    Ok((code, buf[4..].to_vec()))
}
