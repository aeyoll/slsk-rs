//! Protocol constants and shared types.

// ── Connection types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionType {
    /// Peer-to-peer content (searches, user info, queuing)
    PeerToPeer,
    /// File transfer
    FileTransfer,
    /// Distributed search network
    Distributed,
}

impl ConnectionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionType::PeerToPeer => "P",
            ConnectionType::FileTransfer => "F",
            ConnectionType::Distributed => "D",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "P" => Some(ConnectionType::PeerToPeer),
            "F" => Some(ConnectionType::FileTransfer),
            "D" => Some(ConnectionType::Distributed),
            _ => None,
        }
    }
}

// ── User status ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum UserStatus {
    Offline = 0,
    Away = 1,
    Online = 2,
}

impl TryFrom<u32> for UserStatus {
    type Error = crate::error::Error;
    fn try_from(v: u32) -> crate::error::Result<Self> {
        match v {
            0 => Ok(UserStatus::Offline),
            1 => Ok(UserStatus::Away),
            2 => Ok(UserStatus::Online),
            _ => Err(crate::error::Error::Protocol(format!(
                "unknown user status {}",
                v
            ))),
        }
    }
}

// ── Upload permissions ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum UploadPermission {
    NoOne = 0,
    Everyone = 1,
    UsersInList = 2,
    PermittedUsers = 3,
}

impl TryFrom<u32> for UploadPermission {
    type Error = crate::error::Error;
    fn try_from(v: u32) -> crate::error::Result<Self> {
        match v {
            0 => Ok(UploadPermission::NoOne),
            1 => Ok(UploadPermission::Everyone),
            2 => Ok(UploadPermission::UsersInList),
            3 => Ok(UploadPermission::PermittedUsers),
            _ => Err(crate::error::Error::Protocol(format!(
                "unknown upload permission {}",
                v
            ))),
        }
    }
}

// ── Transfer direction ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TransferDirection {
    Download = 0,
    Upload = 1,
}

impl TryFrom<u32> for TransferDirection {
    type Error = crate::error::Error;
    fn try_from(v: u32) -> crate::error::Result<Self> {
        match v {
            0 => Ok(TransferDirection::Download),
            1 => Ok(TransferDirection::Upload),
            _ => Err(crate::error::Error::Protocol(format!(
                "unknown transfer direction {}",
                v
            ))),
        }
    }
}

// ── Obfuscation ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum ObfuscationType {
    #[default]
    None = 0,
    Rotated = 1,
}

impl TryFrom<u32> for ObfuscationType {
    type Error = crate::error::Error;
    fn try_from(v: u32) -> crate::error::Result<Self> {
        match v {
            0 => Ok(ObfuscationType::None),
            1 => Ok(ObfuscationType::Rotated),
            _ => Err(crate::error::Error::Protocol(format!(
                "unknown obfuscation type {}",
                v
            ))),
        }
    }
}

// ── File attributes ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum FileAttributeType {
    Bitrate = 0,
    Duration = 1,
    Vbr = 2,
    Encoder = 3,
    SampleRate = 4,
    BitDepth = 5,
}

impl TryFrom<u32> for FileAttributeType {
    type Error = crate::error::Error;
    fn try_from(v: u32) -> crate::error::Result<Self> {
        match v {
            0 => Ok(FileAttributeType::Bitrate),
            1 => Ok(FileAttributeType::Duration),
            2 => Ok(FileAttributeType::Vbr),
            3 => Ok(FileAttributeType::Encoder),
            4 => Ok(FileAttributeType::SampleRate),
            5 => Ok(FileAttributeType::BitDepth),
            _ => Err(crate::error::Error::Protocol(format!(
                "unknown file attribute {}",
                v
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileAttribute {
    pub code: FileAttributeType,
    pub value: u32,
}

// ── Shared file entry ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SharedFile {
    /// Always 1 in practice
    pub code: u8,
    pub filename: String,
    pub size: u64,
    pub extension: String,
    pub attributes: Vec<FileAttribute>,
}

// ── User stats ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct UserStats {
    pub avg_speed: u32,
    pub upload_num: u32,
    pub unknown: u32,
    pub files: u32,
    pub dirs: u32,
}
