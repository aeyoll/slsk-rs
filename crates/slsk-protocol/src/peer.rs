//! Peer messages — sent over a "P" (peer-to-peer) TCP connection.
//! Uses uint32 message codes with the standard length-prefixed framing.

use std::io::Cursor;

use crate::{
    codec::*,
    error::Result,
    types::{FileAttribute, FileAttributeType, TransferDirection, UploadPermission},
};

pub mod code {
    pub const GET_SHARED_FILE_LIST: u32 = 4;
    pub const SHARED_FILE_LIST_RESPONSE: u32 = 5;
    pub const FILE_SEARCH_RESPONSE: u32 = 9;
    pub const USER_INFO_REQUEST: u32 = 15;
    pub const USER_INFO_RESPONSE: u32 = 16;
    pub const FOLDER_CONTENTS_REQUEST: u32 = 36;
    pub const FOLDER_CONTENTS_RESPONSE: u32 = 37;
    pub const TRANSFER_REQUEST: u32 = 40;
    pub const TRANSFER_RESPONSE: u32 = 41;
    pub const QUEUE_UPLOAD: u32 = 43;
    pub const PLACE_IN_QUEUE_RESPONSE: u32 = 44;
    pub const UPLOAD_FAILED: u32 = 46;
    pub const UPLOAD_DENIED: u32 = 50;
    pub const PLACE_IN_QUEUE_REQUEST: u32 = 51;
}

// ── GetSharedFileList ─────────────────────────────────────────────────────────

/// Empty message — request the peer's full share list.
pub fn encode_get_shared_file_list() -> Vec<u8> {
    frame_message_u32(code::GET_SHARED_FILE_LIST, &[])
}

/// Empty message — request the peer to send us their user info.
pub fn encode_user_info_request() -> Vec<u8> {
    frame_message_u32(code::USER_INFO_REQUEST, &[])
}

// ── SharedFileListResponse ────────────────────────────────────────────────────

pub use crate::shared_files::SharedFileListResponse as SharedFileListResponseData;

/// Encode a shared file list response (already-compressed payload).
pub fn encode_shared_file_list_response(compressed: &[u8]) -> Vec<u8> {
    frame_message_u32(code::SHARED_FILE_LIST_RESPONSE, compressed)
}

// ── FileSearchResponse ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FileSearchResult {
    pub code: u8,
    pub filename: String,
    pub size: u64,
    pub extension: String,
    pub attributes: Vec<FileAttribute>,
}

#[derive(Debug, Clone)]
pub struct FileSearchResponse {
    pub username: String,
    pub token: u32,
    pub results: Vec<FileSearchResult>,
    pub slot_free: bool,
    pub avg_speed: u32,
    pub queue_length: u32,
    pub unknown: u32,
    pub private_results: Vec<FileSearchResult>,
}

fn decode_search_result(cur: &mut Cursor<&[u8]>) -> Result<FileSearchResult> {
    let code = read_u8(cur)?;
    let filename = read_string(cur)?;
    let size = read_u64_le(cur)?;
    let extension = read_string(cur)?;
    let n = read_u32_le(cur)? as usize;
    let mut attributes = Vec::with_capacity(n);
    for _ in 0..n {
        let ac = read_u32_le(cur)?;
        let av = read_u32_le(cur)?;
        if let Ok(t) = FileAttributeType::try_from(ac) {
            attributes.push(FileAttribute { code: t, value: av });
        }
    }
    Ok(FileSearchResult {
        code,
        filename,
        size,
        extension,
        attributes,
    })
}

fn encode_search_result(r: &FileSearchResult, out: &mut Vec<u8>) {
    write_u8(out, r.code).unwrap();
    write_string(out, &r.filename).unwrap();
    write_u64_le(out, r.size).unwrap();
    write_string(out, &r.extension).unwrap();
    write_u32_le(out, r.attributes.len() as u32).unwrap();
    for a in &r.attributes {
        write_u32_le(out, a.code as u32).unwrap();
        write_u32_le(out, a.value).unwrap();
    }
}

impl FileSearchResponse {
    pub fn decode_compressed(data: &[u8]) -> Result<Self> {
        use crate::error::Error;
        use flate2::read::ZlibDecoder;
        use std::io::Read;

        let mut dec = ZlibDecoder::new(data);
        let mut raw = Vec::new();
        dec.read_to_end(&mut raw)
            .map_err(|e| Error::Zlib(e.to_string()))?;

        let slice = raw.as_slice();
        let mut cur = Cursor::new(slice);
        let username = read_string(&mut cur)?;
        let token = read_u32_le(&mut cur)?;
        let n = read_u32_le(&mut cur)? as usize;
        let mut results = Vec::with_capacity(n);
        for _ in 0..n {
            results.push(decode_search_result(&mut cur)?);
        }
        let slot_free = read_bool(&mut cur)?;
        let avg_speed = read_u32_le(&mut cur)?;
        let queue_length = read_u32_le(&mut cur)?;
        let unknown = read_u32_le(&mut cur)?;
        let np = read_u32_le(&mut cur).unwrap_or(0) as usize;
        let mut private_results = Vec::with_capacity(np);
        for _ in 0..np {
            if let Ok(r) = decode_search_result(&mut cur) {
                private_results.push(r);
            }
        }
        Ok(Self {
            username,
            token,
            results,
            slot_free,
            avg_speed,
            queue_length,
            unknown,
            private_results,
        })
    }

    pub fn encode_compressed(&self) -> Result<Vec<u8>> {
        use crate::error::Error;
        use flate2::{Compression, write::ZlibEncoder};
        use std::io::Write;

        let mut raw = Vec::new();
        write_string(&mut raw, &self.username).unwrap();
        write_u32_le(&mut raw, self.token).unwrap();
        write_u32_le(&mut raw, self.results.len() as u32).unwrap();
        for r in &self.results {
            encode_search_result(r, &mut raw);
        }
        write_bool(&mut raw, self.slot_free).unwrap();
        write_u32_le(&mut raw, self.avg_speed).unwrap();
        write_u32_le(&mut raw, self.queue_length).unwrap();
        write_u32_le(&mut raw, self.unknown).unwrap();
        write_u32_le(&mut raw, self.private_results.len() as u32).unwrap();
        for r in &self.private_results {
            encode_search_result(r, &mut raw);
        }

        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw)
            .map_err(|e| Error::Zlib(e.to_string()))?;
        enc.finish().map_err(|e| Error::Zlib(e.to_string()))
    }
}

// ── UserInfoResponse ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct UserInfoResponse {
    pub description: String,
    pub picture: Option<Vec<u8>>,
    pub total_uploads: u32,
    pub queue_size: u32,
    pub slots_free: bool,
    /// Upload permission setting (optional, not sent by SoulseekQt)
    pub upload_permitted: Option<UploadPermission>,
}

impl UserInfoResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let description = read_string(&mut cur)?;
        let has_picture = read_bool(&mut cur)?;
        let picture = if has_picture {
            Some(read_bytes(&mut cur)?)
        } else {
            None
        };
        let total_uploads = read_u32_le(&mut cur)?;
        let queue_size = read_u32_le(&mut cur)?;
        let slots_free = read_bool(&mut cur)?;
        let upload_permitted = read_u32_le(&mut cur)
            .ok()
            .and_then(|v| UploadPermission::try_from(v).ok());
        Ok(Self {
            description,
            picture,
            total_uploads,
            queue_size,
            slots_free,
            upload_permitted,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.description).unwrap();
        match &self.picture {
            Some(pic) => {
                write_bool(&mut body, true).unwrap();
                write_bytes(&mut body, pic).unwrap();
            }
            None => {
                write_bool(&mut body, false).unwrap();
            }
        }
        write_u32_le(&mut body, self.total_uploads).unwrap();
        write_u32_le(&mut body, self.queue_size).unwrap();
        write_bool(&mut body, self.slots_free).unwrap();
        if let Some(perm) = self.upload_permitted {
            write_u32_le(&mut body, perm as u32).unwrap();
        }
        frame_message_u32(code::USER_INFO_RESPONSE, &body)
    }
}

// ── FolderContentsRequest ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FolderContentsRequest {
    pub token: u32,
    pub folder: String,
}

impl FolderContentsRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.token).unwrap();
        write_string(&mut body, &self.folder).unwrap();
        frame_message_u32(code::FOLDER_CONTENTS_REQUEST, &body)
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        Ok(Self {
            token: read_u32_le(&mut cur)?,
            folder: read_string(&mut cur)?,
        })
    }
}

// ── TransferRequest ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TransferRequest {
    pub direction: TransferDirection,
    pub token: u32,
    pub filename: String,
    /// Present only when direction == Upload
    pub file_size: Option<u64>,
}

impl TransferRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.direction as u32).unwrap();
        write_u32_le(&mut body, self.token).unwrap();
        write_string(&mut body, &self.filename).unwrap();
        if self.direction == TransferDirection::Upload {
            if let Some(sz) = self.file_size {
                write_u64_le(&mut body, sz).unwrap();
            }
        }
        frame_message_u32(code::TRANSFER_REQUEST, &body)
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let dir_raw = read_u32_le(&mut cur)?;
        let direction = TransferDirection::try_from(dir_raw)?;
        let token = read_u32_le(&mut cur)?;
        let filename = read_string(&mut cur)?;
        let file_size = if direction == TransferDirection::Upload {
            read_u64_le(&mut cur).ok()
        } else {
            None
        };
        Ok(Self {
            direction,
            token,
            filename,
            file_size,
        })
    }
}

// ── TransferResponse ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum TransferResponse {
    /// Upload response: accepted
    UploadAllowed { token: u32 },
    /// Upload response: rejected
    UploadDenied { token: u32, reason: String },
    /// Download response (deprecated): accepted with file size
    DownloadAllowed { token: u32, file_size: u64 },
    /// Download response (deprecated): rejected
    DownloadDenied { token: u32, reason: String },
}

impl TransferResponse {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        match self {
            Self::UploadAllowed { token } => {
                write_u32_le(&mut body, *token).unwrap();
                write_bool(&mut body, true).unwrap();
            }
            Self::UploadDenied { token, reason } => {
                write_u32_le(&mut body, *token).unwrap();
                write_bool(&mut body, false).unwrap();
                write_string(&mut body, reason).unwrap();
            }
            Self::DownloadAllowed { token, file_size } => {
                write_u32_le(&mut body, *token).unwrap();
                write_bool(&mut body, true).unwrap();
                write_u64_le(&mut body, *file_size).unwrap();
            }
            Self::DownloadDenied { token, reason } => {
                write_u32_le(&mut body, *token).unwrap();
                write_bool(&mut body, false).unwrap();
                write_string(&mut body, reason).unwrap();
            }
        }
        frame_message_u32(code::TRANSFER_RESPONSE, &body)
    }

    /// Decode as an upload response (the common path for modern clients).
    pub fn decode_upload(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let token = read_u32_le(&mut cur)?;
        let allowed = read_bool(&mut cur)?;
        if allowed {
            Ok(Self::UploadAllowed { token })
        } else {
            Ok(Self::UploadDenied {
                token,
                reason: read_string(&mut cur)?,
            })
        }
    }

    /// Decode as a download response (legacy support).
    pub fn decode_download(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let token = read_u32_le(&mut cur)?;
        let allowed = read_bool(&mut cur)?;
        if allowed {
            Ok(Self::DownloadAllowed {
                token,
                file_size: read_u64_le(&mut cur)?,
            })
        } else {
            Ok(Self::DownloadDenied {
                token,
                reason: read_string(&mut cur)?,
            })
        }
    }
}

// ── QueueUpload ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct QueueUpload {
    pub filename: String,
}

impl QueueUpload {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.filename).unwrap();
        frame_message_u32(code::QUEUE_UPLOAD, &body)
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        Ok(Self {
            filename: read_string(&mut Cursor::new(data))?,
        })
    }
}

// ── PlaceInQueue ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PlaceInQueueResponse {
    pub filename: String,
    pub place: u32,
}

impl PlaceInQueueResponse {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.filename).unwrap();
        write_u32_le(&mut body, self.place).unwrap();
        frame_message_u32(code::PLACE_IN_QUEUE_RESPONSE, &body)
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        Ok(Self {
            filename: read_string(&mut cur)?,
            place: read_u32_le(&mut cur)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PlaceInQueueRequest {
    pub filename: String,
}

impl PlaceInQueueRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.filename).unwrap();
        frame_message_u32(code::PLACE_IN_QUEUE_REQUEST, &body)
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        Ok(Self {
            filename: read_string(&mut Cursor::new(data))?,
        })
    }
}

// ── UploadFailed ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct UploadFailed {
    pub filename: String,
}

impl UploadFailed {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.filename).unwrap();
        frame_message_u32(code::UPLOAD_FAILED, &body)
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        Ok(Self {
            filename: read_string(&mut Cursor::new(data))?,
        })
    }
}

// ── UploadDenied ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct UploadDenied {
    pub filename: String,
    pub reason: String,
}

impl UploadDenied {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.filename).unwrap();
        write_string(&mut body, &self.reason).unwrap();
        frame_message_u32(code::UPLOAD_DENIED, &body)
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        Ok(Self {
            filename: read_string(&mut cur)?,
            reason: read_string(&mut cur)?,
        })
    }
}

// ── Top-level enum ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PeerMessage {
    GetSharedFileList,
    SharedFileListResponse(Vec<u8>), // raw compressed bytes — use SharedFileListResponse::decode_compressed
    FileSearchResponse(Vec<u8>), // raw compressed bytes — use FileSearchResponse::decode_compressed
    UserInfoRequest,
    UserInfoResponse(UserInfoResponse),
    FolderContentsRequest(FolderContentsRequest),
    FolderContentsResponse(Vec<u8>), // raw compressed bytes — use FolderContentsResponse::decode_compressed
    TransferRequest(TransferRequest),
    TransferResponse(Vec<u8>), // raw bytes — use TransferResponse::decode_upload / decode_download
    QueueUpload(QueueUpload),
    PlaceInQueueResponse(PlaceInQueueResponse),
    UploadFailed(UploadFailed),
    UploadDenied(UploadDenied),
    PlaceInQueueRequest(PlaceInQueueRequest),
    Unknown { code: u32, body: Vec<u8> },
}

impl PeerMessage {
    pub fn decode(code: u32, body: &[u8]) -> Result<Self> {
        match code {
            code::GET_SHARED_FILE_LIST => Ok(Self::GetSharedFileList),
            code::SHARED_FILE_LIST_RESPONSE => Ok(Self::SharedFileListResponse(body.to_vec())),
            code::FILE_SEARCH_RESPONSE => Ok(Self::FileSearchResponse(body.to_vec())),
            code::USER_INFO_REQUEST => Ok(Self::UserInfoRequest),
            code::USER_INFO_RESPONSE => Ok(Self::UserInfoResponse(UserInfoResponse::decode(body)?)),
            code::FOLDER_CONTENTS_REQUEST => Ok(Self::FolderContentsRequest(
                FolderContentsRequest::decode(body)?,
            )),
            code::FOLDER_CONTENTS_RESPONSE => Ok(Self::FolderContentsResponse(body.to_vec())),
            code::TRANSFER_REQUEST => Ok(Self::TransferRequest(TransferRequest::decode(body)?)),
            code::TRANSFER_RESPONSE => Ok(Self::TransferResponse(body.to_vec())),
            code::QUEUE_UPLOAD => Ok(Self::QueueUpload(QueueUpload::decode(body)?)),
            code::PLACE_IN_QUEUE_RESPONSE => Ok(Self::PlaceInQueueResponse(
                PlaceInQueueResponse::decode(body)?,
            )),
            code::UPLOAD_FAILED => Ok(Self::UploadFailed(UploadFailed::decode(body)?)),
            code::UPLOAD_DENIED => Ok(Self::UploadDenied(UploadDenied::decode(body)?)),
            code::PLACE_IN_QUEUE_REQUEST => Ok(Self::PlaceInQueueRequest(
                PlaceInQueueRequest::decode(body)?,
            )),
            _ => Ok(Self::Unknown {
                code,
                body: body.to_vec(),
            }),
        }
    }
}
