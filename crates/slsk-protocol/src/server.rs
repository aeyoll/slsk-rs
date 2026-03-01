//! Server messages (TCP, uint32 code framing).
//!
//! Each variant has a `decode(body: &[u8])` and `encode(&self) -> Vec<u8>` impl.

use std::io::Cursor;

use crate::{
    codec::*,
    error::{Error, Result},
    peer::FileSearchResponse,
    types::*,
};

// ── Code constants ────────────────────────────────────────────────────────────

pub mod code {
    pub const LOGIN: u32 = 1;
    pub const SET_WAIT_PORT: u32 = 2;
    pub const GET_PEER_ADDRESS: u32 = 3;
    pub const WATCH_USER: u32 = 5;
    pub const UNWATCH_USER: u32 = 6;
    pub const GET_USER_STATUS: u32 = 7;
    pub const SAY_CHATROOM: u32 = 13;
    pub const JOIN_ROOM: u32 = 14;
    pub const LEAVE_ROOM: u32 = 15;
    pub const USER_JOINED_ROOM: u32 = 16;
    pub const USER_LEFT_ROOM: u32 = 17;
    pub const CONNECT_TO_PEER: u32 = 18;
    pub const MESSAGE_USER: u32 = 22;
    pub const MESSAGE_ACKED: u32 = 23;
    pub const FILE_SEARCH: u32 = 26;
    pub const SET_STATUS: u32 = 28;
    pub const SERVER_PING: u32 = 32;
    pub const SHARED_FOLDERS_FILES: u32 = 35;
    pub const GET_USER_STATS: u32 = 36;
    pub const RELOGGED: u32 = 41;
    pub const USER_SEARCH: u32 = 42;
    pub const ADD_THING_I_LIKE: u32 = 51;
    pub const REMOVE_THING_I_LIKE: u32 = 52;
    pub const RECOMMENDATIONS: u32 = 54;
    pub const GLOBAL_RECOMMENDATIONS: u32 = 56;
    pub const USER_INTERESTS: u32 = 57;
    pub const ROOM_LIST: u32 = 64;
    pub const ADMIN_MESSAGE: u32 = 66;
    pub const PRIVILEGED_USERS: u32 = 69;
    pub const HAVE_NO_PARENT: u32 = 71;
    pub const PARENT_IP: u32 = 73;
    pub const PARENT_MIN_SPEED: u32 = 83;
    pub const PARENT_SPEED_RATIO: u32 = 84;
    pub const CHECK_PRIVILEGES: u32 = 92;
    pub const EMBEDDED_MESSAGE: u32 = 93;
    pub const ACCEPT_CHILDREN: u32 = 100;
    pub const POSSIBLE_PARENTS: u32 = 102;
    pub const WISHLIST_SEARCH: u32 = 103;
    pub const WISHLIST_INTERVAL: u32 = 104;
    pub const SIMILAR_USERS: u32 = 110;
    pub const ITEM_RECOMMENDATIONS: u32 = 111;
    pub const ITEM_SIMILAR_USERS: u32 = 112;
    pub const ROOM_TICKER_STATE: u32 = 113;
    pub const ROOM_TICKER_ADD: u32 = 114;
    pub const ROOM_TICKER_REMOVE: u32 = 115;
    pub const ROOM_TICKER_SET: u32 = 116;
    pub const ADD_THING_I_HATE: u32 = 117;
    pub const REMOVE_THING_I_HATE: u32 = 118;
    pub const ROOM_SEARCH: u32 = 120;
    pub const SEND_UPLOAD_SPEED: u32 = 121;
    pub const GIVE_PRIVILEGES: u32 = 123;
    pub const BRANCH_LEVEL: u32 = 126;
    pub const BRANCH_ROOT: u32 = 127;
    pub const RESET_DISTRIBUTED: u32 = 130;
    pub const ROOM_MEMBERS: u32 = 133;
    pub const ADD_ROOM_MEMBER: u32 = 134;
    pub const REMOVE_ROOM_MEMBER: u32 = 135;
    pub const CANCEL_ROOM_MEMBERSHIP: u32 = 136;
    pub const CANCEL_ROOM_OWNERSHIP: u32 = 137;
    pub const ROOM_MEMBERSHIP_GRANTED: u32 = 139;
    pub const ROOM_MEMBERSHIP_REVOKED: u32 = 140;
    pub const ENABLE_ROOM_INVITATIONS: u32 = 141;
    pub const CHANGE_PASSWORD: u32 = 142;
    pub const ADD_ROOM_OPERATOR: u32 = 143;
    pub const REMOVE_ROOM_OPERATOR: u32 = 144;
    pub const ROOM_OPERATORSHIP_GRANTED: u32 = 145;
    pub const ROOM_OPERATORSHIP_REVOKED: u32 = 146;
    pub const ROOM_OPERATORS: u32 = 148;
    pub const MESSAGE_USERS: u32 = 149;
    pub const JOIN_GLOBAL_ROOM: u32 = 150;
    pub const LEAVE_GLOBAL_ROOM: u32 = 151;
    pub const GLOBAL_ROOM_MESSAGE: u32 = 152;
    pub const EXCLUDED_SEARCH_PHRASES: u32 = 160;
    pub const CANT_CONNECT_TO_PEER: u32 = 1001;
    pub const CANT_CREATE_ROOM: u32 = 1003;
}

// ── Login ─────────────────────────────────────────────────────────────────────

/// Compute the MD5 hex digest used in the login message.
pub fn login_hash(username: &str, password: &str) -> String {
    let digest = md5::compute(format!("{}{}", username, password));
    format!("{:x}", digest)
}

/// Compute the password-only MD5 hash returned in the login response.
pub fn password_hash(password: &str) -> String {
    let digest = md5::compute(password);
    format!("{:x}", digest)
}

#[derive(Debug, Clone)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Protocol version — use 160 for Nicotine+
    pub version: u32,
    /// Minor version
    pub minor_version: u32,
}

impl LoginRequest {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            version: 160,
            minor_version: 1,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let hash = login_hash(&self.username, &self.password);
        let mut body = Vec::new();
        write_string(&mut body, &self.username).unwrap();
        write_string(&mut body, &self.password).unwrap();
        write_u32_le(&mut body, self.version).unwrap();
        write_string(&mut body, &hash).unwrap();
        write_u32_le(&mut body, self.minor_version).unwrap();
        frame_message_u32(code::LOGIN, &body)
    }
}

#[derive(Debug, Clone)]
pub enum LoginResponse {
    Success {
        greet: String,
        own_ip: u32,
        password_hash: String,
        is_supporter: bool,
    },
    Failure {
        reason: String,
        detail: Option<String>,
    },
}

impl LoginResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let success = read_bool(&mut cur)?;
        if success {
            let greet = read_string(&mut cur)?;
            let own_ip = read_u32_le(&mut cur)?;
            let pw_hash = read_string(&mut cur)?;
            let is_supporter = read_bool(&mut cur)?;
            Ok(LoginResponse::Success {
                greet,
                own_ip,
                password_hash: pw_hash,
                is_supporter,
            })
        } else {
            let reason = read_string(&mut cur)?;
            let detail = if reason == "INVALIDUSERNAME" {
                Some(read_string(&mut cur)?)
            } else {
                None
            };
            Ok(LoginResponse::Failure { reason, detail })
        }
    }
}

// ── SetWaitPort ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SetWaitPort {
    pub port: u32,
    pub obfuscation_type: Option<u32>,
    pub obfuscated_port: Option<u32>,
}

impl SetWaitPort {
    pub fn new(port: u32) -> Self {
        Self {
            port,
            obfuscation_type: None,
            obfuscated_port: None,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.port).unwrap();
        if let (Some(ot), Some(op)) = (self.obfuscation_type, self.obfuscated_port) {
            write_u32_le(&mut body, ot).unwrap();
            write_u32_le(&mut body, op).unwrap();
        }
        frame_message_u32(code::SET_WAIT_PORT, &body)
    }
}

// ── GetPeerAddress ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GetPeerAddressRequest {
    pub username: String,
}

impl GetPeerAddressRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.username).unwrap();
        frame_message_u32(code::GET_PEER_ADDRESS, &body)
    }
}

#[derive(Debug, Clone)]
pub struct GetPeerAddressResponse {
    pub username: String,
    pub ip: u32,
    pub port: u32,
    pub obfuscation_type: u32,
    pub obfuscated_port: u16,
}

impl GetPeerAddressResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            username: read_string(&mut cur)?,
            ip: read_u32_le(&mut cur)?,
            port: read_u32_le(&mut cur)?,
            obfuscation_type: read_u32_le(&mut cur)?,
            obfuscated_port: read_u16_le(&mut cur)?,
        })
    }
}

// ── WatchUser / UnwatchUser ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WatchUserRequest {
    pub username: String,
}

impl WatchUserRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.username).unwrap();
        frame_message_u32(code::WATCH_USER, &body)
    }
}

#[derive(Debug, Clone)]
pub struct WatchUserResponse {
    pub username: String,
    pub exists: bool,
    pub status: Option<UserStatus>,
    pub stats: Option<UserStats>,
    pub country_code: Option<String>,
}

impl WatchUserResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let username = read_string(&mut cur)?;
        let exists = read_bool(&mut cur)?;
        if exists {
            let status = UserStatus::try_from(read_u32_le(&mut cur)?)?;
            let avg_speed = read_u32_le(&mut cur)?;
            let upload_num = read_u32_le(&mut cur)?;
            let unknown = read_u32_le(&mut cur)?;
            let files = read_u32_le(&mut cur)?;
            let dirs = read_u32_le(&mut cur)?;
            let country_code = match status {
                UserStatus::Offline => None,
                _ => Some(read_string(&mut cur)?),
            };
            Ok(WatchUserResponse {
                username,
                exists: true,
                status: Some(status),
                stats: Some(UserStats {
                    avg_speed,
                    upload_num,
                    unknown,
                    files,
                    dirs,
                }),
                country_code,
            })
        } else {
            Ok(WatchUserResponse {
                username,
                exists: false,
                status: None,
                stats: None,
                country_code: None,
            })
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnwatchUser {
    pub username: String,
}

impl UnwatchUser {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.username).unwrap();
        frame_message_u32(code::UNWATCH_USER, &body)
    }
}

// ── GetUserStatus ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GetUserStatusRequest {
    pub username: String,
}

impl GetUserStatusRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.username).unwrap();
        frame_message_u32(code::GET_USER_STATUS, &body)
    }
}

#[derive(Debug, Clone)]
pub struct GetUserStatusResponse {
    pub username: String,
    pub status: UserStatus,
    pub privileged: bool,
}

impl GetUserStatusResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            username: read_string(&mut cur)?,
            status: UserStatus::try_from(read_u32_le(&mut cur)?)?,
            privileged: read_bool(&mut cur)?,
        })
    }
}

// ── SayChatroom ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SayChatroomSend {
    pub room: String,
    pub message: String,
}

impl SayChatroomSend {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.room).unwrap();
        write_string(&mut body, &self.message).unwrap();
        frame_message_u32(code::SAY_CHATROOM, &body)
    }
}

#[derive(Debug, Clone)]
pub struct SayChatroomRecv {
    pub room: String,
    pub username: String,
    pub message: String,
}

impl SayChatroomRecv {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            room: read_string(&mut cur)?,
            username: read_string(&mut cur)?,
            message: read_string(&mut cur)?,
        })
    }
}

// ── JoinRoom ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct JoinRoomRequest {
    pub room: String,
    /// 1 = private, 0 = public
    pub private: u32,
}

impl JoinRoomRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.room).unwrap();
        write_u32_le(&mut body, self.private).unwrap();
        frame_message_u32(code::JOIN_ROOM, &body)
    }
}

#[derive(Debug, Clone)]
pub struct RoomUserEntry {
    pub username: String,
    pub status: u32,
    pub stats: UserStats,
    pub slots_full: u32,
    pub country_code: String,
}

#[derive(Debug, Clone)]
pub struct JoinRoomResponse {
    pub room: String,
    pub users: Vec<RoomUserEntry>,
    pub owner: Option<String>,
    pub operators: Vec<String>,
}

impl JoinRoomResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let room = read_string(&mut cur)?;
        let n = read_u32_le(&mut cur)? as usize;
        let mut usernames = Vec::with_capacity(n);
        for _ in 0..n {
            usernames.push(read_string(&mut cur)?);
        }
        let ns = read_u32_le(&mut cur)? as usize;
        let mut statuses = Vec::with_capacity(ns);
        for _ in 0..ns {
            statuses.push(read_u32_le(&mut cur)?);
        }
        let nst = read_u32_le(&mut cur)? as usize;
        let mut stats_list = Vec::with_capacity(nst);
        for _ in 0..nst {
            stats_list.push(UserStats {
                avg_speed: read_u32_le(&mut cur)?,
                upload_num: read_u32_le(&mut cur)?,
                unknown: read_u32_le(&mut cur)?,
                files: read_u32_le(&mut cur)?,
                dirs: read_u32_le(&mut cur)?,
            });
        }
        let nsf = read_u32_le(&mut cur)? as usize;
        let mut slots_full = Vec::with_capacity(nsf);
        for _ in 0..nsf {
            slots_full.push(read_u32_le(&mut cur)?);
        }
        let ncc = read_u32_le(&mut cur)? as usize;
        let mut country_codes = Vec::with_capacity(ncc);
        for _ in 0..ncc {
            country_codes.push(read_string(&mut cur)?);
        }

        let mut users = Vec::new();
        for i in 0..n {
            users.push(RoomUserEntry {
                username: usernames[i].clone(),
                status: *statuses.get(i).unwrap_or(&0),
                stats: stats_list.get(i).cloned().unwrap_or_default(),
                slots_full: *slots_full.get(i).unwrap_or(&0),
                country_code: country_codes.get(i).cloned().unwrap_or_default(),
            });
        }

        // Optional private room info
        let owner = read_string(&mut cur).ok();
        let mut operators = Vec::new();
        if let Ok(nop) = read_u32_le(&mut cur) {
            for _ in 0..nop {
                if let Ok(op) = read_string(&mut cur) {
                    operators.push(op);
                }
            }
        }

        Ok(JoinRoomResponse {
            room,
            users,
            owner,
            operators,
        })
    }
}

// ── LeaveRoom ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LeaveRoom {
    pub room: String,
}

impl LeaveRoom {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.room).unwrap();
        frame_message_u32(code::LEAVE_ROOM, &body)
    }
}

// ── UserJoinedRoom ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct UserJoinedRoom {
    pub room: String,
    pub username: String,
    pub status: u32,
    pub stats: UserStats,
    pub slots_full: u32,
    pub country_code: String,
}

impl UserJoinedRoom {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            room: read_string(&mut cur)?,
            username: read_string(&mut cur)?,
            status: read_u32_le(&mut cur)?,
            stats: UserStats {
                avg_speed: read_u32_le(&mut cur)?,
                upload_num: read_u32_le(&mut cur)?,
                unknown: read_u32_le(&mut cur)?,
                files: read_u32_le(&mut cur)?,
                dirs: read_u32_le(&mut cur)?,
            },
            slots_full: read_u32_le(&mut cur)?,
            country_code: read_string(&mut cur)?,
        })
    }
}

// ── UserLeftRoom ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct UserLeftRoom {
    pub room: String,
    pub username: String,
}

impl UserLeftRoom {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            room: read_string(&mut cur)?,
            username: read_string(&mut cur)?,
        })
    }
}

// ── ConnectToPeer ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ConnectToPeerRequest {
    pub token: u32,
    pub username: String,
    pub conn_type: String,
}

impl ConnectToPeerRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.token).unwrap();
        write_string(&mut body, &self.username).unwrap();
        write_string(&mut body, &self.conn_type).unwrap();
        frame_message_u32(code::CONNECT_TO_PEER, &body)
    }
}

#[derive(Debug, Clone)]
pub struct ConnectToPeerResponse {
    pub username: String,
    pub conn_type: String,
    pub ip: u32,
    pub port: u32,
    pub token: u32,
    pub privileged: bool,
    pub obfuscation_type: u32,
    pub obfuscated_port: u32,
}

impl ConnectToPeerResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            username: read_string(&mut cur)?,
            conn_type: read_string(&mut cur)?,
            ip: read_u32_le(&mut cur)?,
            port: read_u32_le(&mut cur)?,
            token: read_u32_le(&mut cur)?,
            privileged: read_bool(&mut cur)?,
            obfuscation_type: read_u32_le(&mut cur)?,
            obfuscated_port: read_u32_le(&mut cur)?,
        })
    }
}

// ── MessageUser (private chat) ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MessageUserSend {
    pub username: String,
    pub message: String,
}

impl MessageUserSend {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.username).unwrap();
        write_string(&mut body, &self.message).unwrap();
        frame_message_u32(code::MESSAGE_USER, &body)
    }
}

#[derive(Debug, Clone)]
pub struct MessageUserRecv {
    pub id: u32,
    pub timestamp: u32,
    pub username: String,
    pub message: String,
    pub new_message: bool,
}

impl MessageUserRecv {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            id: read_u32_le(&mut cur)?,
            timestamp: read_u32_le(&mut cur)?,
            username: read_string(&mut cur)?,
            message: read_string(&mut cur)?,
            new_message: read_bool(&mut cur)?,
        })
    }
}

// ── MessageAcked ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MessageAcked {
    pub message_id: u32,
}

impl MessageAcked {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.message_id).unwrap();
        frame_message_u32(code::MESSAGE_ACKED, &body)
    }
}

// ── FileSearch ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FileSearchRequest {
    pub token: u32,
    pub query: String,
}

impl FileSearchRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.token).unwrap();
        write_string(&mut body, &self.query).unwrap();
        frame_message_u32(code::FILE_SEARCH, &body)
    }

    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            token: read_u32_le(&mut cur)?,
            query: read_string(&mut cur)?,
        })
    }
}

// ── SetStatus ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SetStatus {
    pub status: i32,
}

impl SetStatus {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_i32_le(&mut body, self.status).unwrap();
        frame_message_u32(code::SET_STATUS, &body)
    }
}

// ── SharedFoldersFiles ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SharedFoldersFiles {
    pub dirs: u32,
    pub files: u32,
}

impl SharedFoldersFiles {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.dirs).unwrap();
        write_u32_le(&mut body, self.files).unwrap();
        frame_message_u32(code::SHARED_FOLDERS_FILES, &body)
    }
}

// ── GetUserStats ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GetUserStatsRequest {
    pub username: String,
}

impl GetUserStatsRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.username).unwrap();
        frame_message_u32(code::GET_USER_STATS, &body)
    }
}

#[derive(Debug, Clone)]
pub struct GetUserStatsResponse {
    pub username: String,
    pub stats: UserStats,
}

impl GetUserStatsResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            username: read_string(&mut cur)?,
            stats: UserStats {
                avg_speed: read_u32_le(&mut cur)?,
                upload_num: read_u32_le(&mut cur)?,
                unknown: read_u32_le(&mut cur)?,
                files: read_u32_le(&mut cur)?,
                dirs: read_u32_le(&mut cur)?,
            },
        })
    }
}

// ── UserSearch ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct UserSearch {
    pub username: String,
    pub token: u32,
    pub query: String,
}

impl UserSearch {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.username).unwrap();
        write_u32_le(&mut body, self.token).unwrap();
        write_string(&mut body, &self.query).unwrap();
        frame_message_u32(code::USER_SEARCH, &body)
    }
}

// ── Recommendations / interests ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RecommendationEntry {
    pub item: String,
    pub score: i32,
}

#[derive(Debug, Clone)]
pub struct RecommendationsResponse {
    pub recommendations: Vec<RecommendationEntry>,
    pub unrecommendations: Vec<RecommendationEntry>,
}

impl RecommendationsResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let n = read_u32_le(&mut cur)? as usize;
        let mut recommendations = Vec::with_capacity(n);
        for _ in 0..n {
            recommendations.push(RecommendationEntry {
                item: read_string(&mut cur)?,
                score: read_i32_le(&mut cur)?,
            });
        }
        let m = read_u32_le(&mut cur)? as usize;
        let mut unrecommendations = Vec::with_capacity(m);
        for _ in 0..m {
            unrecommendations.push(RecommendationEntry {
                item: read_string(&mut cur)?,
                score: read_i32_le(&mut cur)?,
            });
        }
        Ok(Self {
            recommendations,
            unrecommendations,
        })
    }
}

#[derive(Debug, Clone)]
pub struct UserInterestsResponse {
    pub username: String,
    pub liked: Vec<String>,
    pub hated: Vec<String>,
}

impl UserInterestsResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let username = read_string(&mut cur)?;
        let n_liked = read_u32_le(&mut cur)? as usize;
        let mut liked = Vec::with_capacity(n_liked);
        for _ in 0..n_liked {
            liked.push(read_string(&mut cur)?);
        }
        let n_hated = read_u32_le(&mut cur)? as usize;
        let mut hated = Vec::with_capacity(n_hated);
        for _ in 0..n_hated {
            hated.push(read_string(&mut cur)?);
        }
        Ok(Self {
            username,
            liked,
            hated,
        })
    }
}

// ── RoomList ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RoomEntry {
    pub name: String,
    pub user_count: u32,
}

#[derive(Debug, Clone)]
pub struct RoomListResponse {
    pub public_rooms: Vec<RoomEntry>,
    pub owned_private_rooms: Vec<RoomEntry>,
    pub private_rooms: Vec<RoomEntry>,
    pub operated_private_rooms: Vec<String>,
}

impl RoomListResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);

        let n = read_u32_le(&mut cur)? as usize;
        let mut names = Vec::with_capacity(n);
        for _ in 0..n {
            names.push(read_string(&mut cur)?);
        }
        let n2 = read_u32_le(&mut cur)? as usize;
        let mut counts = vec![0u32; n2];
        for c in counts.iter_mut() {
            *c = read_u32_le(&mut cur)?;
        }
        let public_rooms = names
            .into_iter()
            .zip(counts)
            .map(|(name, user_count)| RoomEntry { name, user_count })
            .collect();

        let no = read_u32_le(&mut cur)? as usize;
        let mut owned_names = Vec::with_capacity(no);
        for _ in 0..no {
            owned_names.push(read_string(&mut cur)?);
        }
        let no2 = read_u32_le(&mut cur)? as usize;
        let mut owned_counts = vec![0u32; no2];
        for c in owned_counts.iter_mut() {
            *c = read_u32_le(&mut cur)?;
        }
        let owned_private_rooms = owned_names
            .into_iter()
            .zip(owned_counts)
            .map(|(name, user_count)| RoomEntry { name, user_count })
            .collect();

        let np = read_u32_le(&mut cur)? as usize;
        let mut priv_names = Vec::with_capacity(np);
        for _ in 0..np {
            priv_names.push(read_string(&mut cur)?);
        }
        let np2 = read_u32_le(&mut cur)? as usize;
        let mut priv_counts = vec![0u32; np2];
        for c in priv_counts.iter_mut() {
            *c = read_u32_le(&mut cur)?;
        }
        let private_rooms = priv_names
            .into_iter()
            .zip(priv_counts)
            .map(|(name, user_count)| RoomEntry { name, user_count })
            .collect();

        let nop = read_u32_le(&mut cur)? as usize;
        let mut operated_private_rooms = Vec::with_capacity(nop);
        for _ in 0..nop {
            operated_private_rooms.push(read_string(&mut cur)?);
        }

        Ok(Self {
            public_rooms,
            owned_private_rooms,
            private_rooms,
            operated_private_rooms,
        })
    }
}

// ── PrivilegedUsers ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PrivilegedUsers {
    pub users: Vec<String>,
}

impl PrivilegedUsers {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let n = read_u32_le(&mut cur)? as usize;
        let mut users = Vec::with_capacity(n);
        for _ in 0..n {
            users.push(read_string(&mut cur)?);
        }
        Ok(Self { users })
    }
}

// ── HaveNoParent ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HaveNoParent {
    pub no_parent: bool,
}

impl HaveNoParent {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_bool(&mut body, self.no_parent).unwrap();
        frame_message_u32(code::HAVE_NO_PARENT, &body)
    }
}

// ── ParentMinSpeed / ParentSpeedRatio ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ParentMinSpeed {
    pub speed: u32,
}
impl ParentMinSpeed {
    pub fn decode(body: &[u8]) -> Result<Self> {
        Ok(Self {
            speed: read_u32_le(&mut Cursor::new(body))?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ParentSpeedRatio {
    pub ratio: u32,
}
impl ParentSpeedRatio {
    pub fn decode(body: &[u8]) -> Result<Self> {
        Ok(Self {
            ratio: read_u32_le(&mut Cursor::new(body))?,
        })
    }
}

// ── CheckPrivileges ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CheckPrivilegesResponse {
    pub time_left: u32,
}
impl CheckPrivilegesResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        Ok(Self {
            time_left: read_u32_le(&mut Cursor::new(body))?,
        })
    }
    pub fn encode_request() -> Vec<u8> {
        frame_message_u32(code::CHECK_PRIVILEGES, &[])
    }
}

// ── EmbeddedMessage ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct EmbeddedMessage {
    pub distributed_code: u8,
    pub distributed_message: Vec<u8>,
}

impl EmbeddedMessage {
    pub fn decode(body: &[u8]) -> Result<Self> {
        if body.is_empty() {
            return Err(Error::Protocol("EmbeddedMessage body is empty".into()));
        }
        Ok(Self {
            distributed_code: body[0],
            distributed_message: body[1..].to_vec(),
        })
    }
}

// ── AcceptChildren ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AcceptChildren {
    pub accept: bool,
}
impl AcceptChildren {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_bool(&mut body, self.accept).unwrap();
        frame_message_u32(code::ACCEPT_CHILDREN, &body)
    }
}

// ── PossibleParents ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PossibleParent {
    pub username: String,
    pub ip: u32,
    pub port: u32,
}

#[derive(Debug, Clone)]
pub struct PossibleParents {
    pub parents: Vec<PossibleParent>,
}

impl PossibleParents {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let n = read_u32_le(&mut cur)? as usize;
        let mut parents = Vec::with_capacity(n);
        for _ in 0..n {
            parents.push(PossibleParent {
                username: read_string(&mut cur)?,
                ip: read_u32_le(&mut cur)?,
                port: read_u32_le(&mut cur)?,
            });
        }
        Ok(Self { parents })
    }
}

// ── WishlistSearch ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WishlistSearch {
    pub token: u32,
    pub query: String,
}
impl WishlistSearch {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.token).unwrap();
        write_string(&mut body, &self.query).unwrap();
        frame_message_u32(code::WISHLIST_SEARCH, &body)
    }
}

#[derive(Debug, Clone)]
pub struct WishlistInterval {
    pub interval: u32,
}
impl WishlistInterval {
    pub fn decode(body: &[u8]) -> Result<Self> {
        Ok(Self {
            interval: read_u32_le(&mut Cursor::new(body))?,
        })
    }
}

// ── SimilarUsers ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SimilarUser {
    pub username: String,
    pub rating: u32,
}

#[derive(Debug, Clone)]
pub struct SimilarUsersResponse {
    pub users: Vec<SimilarUser>,
}
impl SimilarUsersResponse {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let n = read_u32_le(&mut cur)? as usize;
        let mut users = Vec::with_capacity(n);
        for _ in 0..n {
            users.push(SimilarUser {
                username: read_string(&mut cur)?,
                rating: read_u32_le(&mut cur)?,
            });
        }
        Ok(Self { users })
    }
    pub fn encode_request() -> Vec<u8> {
        frame_message_u32(code::SIMILAR_USERS, &[])
    }
}

// ── Room tickers ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RoomTicker {
    pub username: String,
    pub ticker: String,
}

#[derive(Debug, Clone)]
pub struct RoomTickerState {
    pub room: String,
    pub tickers: Vec<RoomTicker>,
}
impl RoomTickerState {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let room = read_string(&mut cur)?;
        let n = read_u32_le(&mut cur)? as usize;
        let mut tickers = Vec::with_capacity(n);
        for _ in 0..n {
            tickers.push(RoomTicker {
                username: read_string(&mut cur)?,
                ticker: read_string(&mut cur)?,
            });
        }
        Ok(Self { room, tickers })
    }
}

#[derive(Debug, Clone)]
pub struct SetRoomTicker {
    pub room: String,
    pub ticker: String,
}
impl SetRoomTicker {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.room).unwrap();
        write_string(&mut body, &self.ticker).unwrap();
        frame_message_u32(code::ROOM_TICKER_SET, &body)
    }
}

// ── RoomSearch ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RoomSearch {
    pub room: String,
    pub token: u32,
    pub query: String,
}
impl RoomSearch {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.room).unwrap();
        write_u32_le(&mut body, self.token).unwrap();
        write_string(&mut body, &self.query).unwrap();
        frame_message_u32(code::ROOM_SEARCH, &body)
    }
}

// ── SendUploadSpeed ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SendUploadSpeed {
    pub speed: u32,
}
impl SendUploadSpeed {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.speed).unwrap();
        frame_message_u32(code::SEND_UPLOAD_SPEED, &body)
    }
}

// ── GivePrivileges ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GivePrivileges {
    pub username: String,
    pub days: u32,
}
impl GivePrivileges {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.username).unwrap();
        write_u32_le(&mut body, self.days).unwrap();
        frame_message_u32(code::GIVE_PRIVILEGES, &body)
    }
}

// ── BranchLevel / BranchRoot ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BranchLevel {
    pub level: u32,
}
impl BranchLevel {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.level).unwrap();
        frame_message_u32(code::BRANCH_LEVEL, &body)
    }
}

#[derive(Debug, Clone)]
pub struct BranchRoot {
    pub root: String,
}
impl BranchRoot {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.root).unwrap();
        frame_message_u32(code::BRANCH_ROOT, &body)
    }
}

// ── Private room management ───────────────────────────────────────────────────

macro_rules! room_member_msg {
    ($name:ident, $code:expr) => {
        #[derive(Debug, Clone)]
        pub struct $name {
            pub room: String,
            pub username: String,
        }
        impl $name {
            pub fn encode(&self) -> Vec<u8> {
                let mut body = Vec::new();
                write_string(&mut body, &self.room).unwrap();
                write_string(&mut body, &self.username).unwrap();
                frame_message_u32($code, &body)
            }
            pub fn decode(body: &[u8]) -> Result<Self> {
                let mut cur = Cursor::new(body);
                Ok(Self {
                    room: read_string(&mut cur)?,
                    username: read_string(&mut cur)?,
                })
            }
        }
    };
}

room_member_msg!(AddRoomMember, code::ADD_ROOM_MEMBER);
room_member_msg!(RemoveRoomMember, code::REMOVE_ROOM_MEMBER);
room_member_msg!(AddRoomOperator, code::ADD_ROOM_OPERATOR);
room_member_msg!(RemoveRoomOperator, code::REMOVE_ROOM_OPERATOR);

#[derive(Debug, Clone)]
pub struct CancelRoomMembership {
    pub room: String,
}
impl CancelRoomMembership {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.room).unwrap();
        frame_message_u32(code::CANCEL_ROOM_MEMBERSHIP, &body)
    }
}

#[derive(Debug, Clone)]
pub struct CancelRoomOwnership {
    pub room: String,
}
impl CancelRoomOwnership {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.room).unwrap();
        frame_message_u32(code::CANCEL_ROOM_OWNERSHIP, &body)
    }
}

#[derive(Debug, Clone)]
pub struct EnableRoomInvitations {
    pub enable: bool,
}
impl EnableRoomInvitations {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_bool(&mut body, self.enable).unwrap();
        frame_message_u32(code::ENABLE_ROOM_INVITATIONS, &body)
    }
    pub fn decode(body: &[u8]) -> Result<Self> {
        Ok(Self {
            enable: read_bool(&mut Cursor::new(body))?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ChangePassword {
    pub password: String,
}
impl ChangePassword {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.password).unwrap();
        frame_message_u32(code::CHANGE_PASSWORD, &body)
    }
    pub fn decode(body: &[u8]) -> Result<Self> {
        Ok(Self {
            password: read_string(&mut Cursor::new(body))?,
        })
    }
}

// ── MessageUsers (broadcast) ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MessageUsers {
    pub usernames: Vec<String>,
    pub message: String,
}
impl MessageUsers {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.usernames.len() as u32).unwrap();
        for u in &self.usernames {
            write_string(&mut body, u).unwrap();
        }
        write_string(&mut body, &self.message).unwrap();
        frame_message_u32(code::MESSAGE_USERS, &body)
    }
}

// ── GlobalRoomMessage ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GlobalRoomMessage {
    pub room: String,
    pub username: String,
    pub message: String,
}
impl GlobalRoomMessage {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            room: read_string(&mut cur)?,
            username: read_string(&mut cur)?,
            message: read_string(&mut cur)?,
        })
    }
}

// ── ExcludedSearchPhrases ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExcludedSearchPhrases {
    pub phrases: Vec<String>,
}
impl ExcludedSearchPhrases {
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let n = read_u32_le(&mut cur)? as usize;
        let mut phrases = Vec::with_capacity(n);
        for _ in 0..n {
            phrases.push(read_string(&mut cur)?);
        }
        Ok(Self { phrases })
    }
}

// ── CantConnectToPeer ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CantConnectToPeerSend {
    pub token: u32,
    pub username: String,
}
impl CantConnectToPeerSend {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.token).unwrap();
        write_string(&mut body, &self.username).unwrap();
        frame_message_u32(code::CANT_CONNECT_TO_PEER, &body)
    }
}

#[derive(Debug, Clone)]
pub struct CantConnectToPeerRecv {
    pub token: u32,
}
impl CantConnectToPeerRecv {
    pub fn decode(body: &[u8]) -> Result<Self> {
        Ok(Self {
            token: read_u32_le(&mut Cursor::new(body))?,
        })
    }
}

// ── AdminMessage ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AdminMessage {
    pub message: String,
}
impl AdminMessage {
    pub fn decode(body: &[u8]) -> Result<Self> {
        Ok(Self {
            message: read_string(&mut Cursor::new(body))?,
        })
    }
}

// ── Top-level incoming server message enum ────────────────────────────────────

/// All possible messages the server sends to the client.
#[derive(Debug, Clone)]
pub enum ServerMessage {
    Login(LoginResponse),
    GetPeerAddress(GetPeerAddressResponse),
    WatchUser(WatchUserResponse),
    GetUserStatus(GetUserStatusResponse),
    SayChatroom(SayChatroomRecv),
    JoinRoom(JoinRoomResponse),
    LeaveRoom(String),
    UserJoinedRoom(UserJoinedRoom),
    UserLeftRoom(UserLeftRoom),
    ConnectToPeer(ConnectToPeerResponse),
    MessageUser(MessageUserRecv),
    FileSearch(FileSearchResponse),
    Relogged,
    GetUserStats(GetUserStatsResponse),
    Recommendations(RecommendationsResponse),
    GlobalRecommendations(RecommendationsResponse),
    UserInterests(UserInterestsResponse),
    RoomList(RoomListResponse),
    AdminMessage(AdminMessage),
    PrivilegedUsers(PrivilegedUsers),
    ParentMinSpeed(ParentMinSpeed),
    ParentSpeedRatio(ParentSpeedRatio),
    CheckPrivileges(CheckPrivilegesResponse),
    EmbeddedMessage(EmbeddedMessage),
    PossibleParents(PossibleParents),
    WishlistInterval(WishlistInterval),
    SimilarUsers(SimilarUsersResponse),
    RoomTickerState(RoomTickerState),
    RoomTickerAdd {
        room: String,
        username: String,
        ticker: String,
    },
    RoomTickerRemove {
        room: String,
        username: String,
    },
    GlobalRoomMessage(GlobalRoomMessage),
    ExcludedSearchPhrases(ExcludedSearchPhrases),
    CantConnectToPeer(CantConnectToPeerRecv),
    CantCreateRoom(String),
    RoomMembershipGranted(String),
    RoomMembershipRevoked(String),
    RoomOperatorshipGranted(String),
    RoomOperatorshipRevoked(String),
    EnableRoomInvitations(EnableRoomInvitations),
    ChangePassword(ChangePassword),
    AddRoomMember(AddRoomMember),
    RemoveRoomMember(RemoveRoomMember),
    AddRoomOperator(AddRoomOperator),
    RemoveRoomOperator(RemoveRoomOperator),
    ResetDistributed,
    /// Unknown/unimplemented message code — raw payload preserved
    Unknown {
        code: u32,
        body: Vec<u8>,
    },
}

impl ServerMessage {
    /// Decode a single server message from a `(code, body)` pair.
    pub fn decode(code: u32, body: &[u8]) -> Result<Self> {
        match code {
            code::LOGIN => Ok(Self::Login(LoginResponse::decode(body)?)),
            code::GET_PEER_ADDRESS => {
                Ok(Self::GetPeerAddress(GetPeerAddressResponse::decode(body)?))
            }
            code::WATCH_USER => Ok(Self::WatchUser(WatchUserResponse::decode(body)?)),
            code::GET_USER_STATUS => Ok(Self::GetUserStatus(GetUserStatusResponse::decode(body)?)),
            code::SAY_CHATROOM => Ok(Self::SayChatroom(SayChatroomRecv::decode(body)?)),
            code::JOIN_ROOM => Ok(Self::JoinRoom(JoinRoomResponse::decode(body)?)),
            code::LEAVE_ROOM => Ok(Self::LeaveRoom(read_string(&mut Cursor::new(body))?)),
            code::USER_JOINED_ROOM => Ok(Self::UserJoinedRoom(
                crate::server::UserJoinedRoom::decode(body)?,
            )),
            code::USER_LEFT_ROOM => Ok(Self::UserLeftRoom(crate::server::UserLeftRoom::decode(
                body,
            )?)),
            code::CONNECT_TO_PEER => Ok(Self::ConnectToPeer(ConnectToPeerResponse::decode(body)?)),
            code::MESSAGE_USER => Ok(Self::MessageUser(MessageUserRecv::decode(body)?)),
            code::FILE_SEARCH => Ok(Self::FileSearch(FileSearchResponse::decode_compressed(
                body,
            )?)),
            code::RELOGGED => Ok(Self::Relogged),
            code::GET_USER_STATS => Ok(Self::GetUserStats(GetUserStatsResponse::decode(body)?)),
            code::RECOMMENDATIONS => Ok(Self::Recommendations(RecommendationsResponse::decode(
                body,
            )?)),
            code::GLOBAL_RECOMMENDATIONS => Ok(Self::GlobalRecommendations(
                RecommendationsResponse::decode(body)?,
            )),
            code::USER_INTERESTS => Ok(Self::UserInterests(UserInterestsResponse::decode(body)?)),
            code::ROOM_LIST => Ok(Self::RoomList(RoomListResponse::decode(body)?)),
            code::ADMIN_MESSAGE => Ok(Self::AdminMessage(crate::server::AdminMessage::decode(
                body,
            )?)),
            code::PRIVILEGED_USERS => Ok(Self::PrivilegedUsers(
                crate::server::PrivilegedUsers::decode(body)?,
            )),
            code::PARENT_MIN_SPEED => Ok(Self::ParentMinSpeed(
                crate::server::ParentMinSpeed::decode(body)?,
            )),
            code::PARENT_SPEED_RATIO => Ok(Self::ParentSpeedRatio(
                crate::server::ParentSpeedRatio::decode(body)?,
            )),
            code::CHECK_PRIVILEGES => Ok(Self::CheckPrivileges(CheckPrivilegesResponse::decode(
                body,
            )?)),
            code::EMBEDDED_MESSAGE => Ok(Self::EmbeddedMessage(
                crate::server::EmbeddedMessage::decode(body)?,
            )),
            code::POSSIBLE_PARENTS => Ok(Self::PossibleParents(PossibleParents::decode(body)?)),
            code::WISHLIST_INTERVAL => Ok(Self::WishlistInterval(WishlistInterval::decode(body)?)),
            code::SIMILAR_USERS => Ok(Self::SimilarUsers(SimilarUsersResponse::decode(body)?)),
            code::ROOM_TICKER_STATE => Ok(Self::RoomTickerState(
                crate::server::RoomTickerState::decode(body)?,
            )),
            code::ROOM_TICKER_ADD => {
                let mut cur = Cursor::new(body);
                Ok(Self::RoomTickerAdd {
                    room: read_string(&mut cur)?,
                    username: read_string(&mut cur)?,
                    ticker: read_string(&mut cur)?,
                })
            }
            code::ROOM_TICKER_REMOVE => {
                let mut cur = Cursor::new(body);
                Ok(Self::RoomTickerRemove {
                    room: read_string(&mut cur)?,
                    username: read_string(&mut cur)?,
                })
            }
            code::GLOBAL_ROOM_MESSAGE => Ok(Self::GlobalRoomMessage(
                crate::server::GlobalRoomMessage::decode(body)?,
            )),
            code::EXCLUDED_SEARCH_PHRASES => Ok(Self::ExcludedSearchPhrases(
                crate::server::ExcludedSearchPhrases::decode(body)?,
            )),
            code::CANT_CONNECT_TO_PEER => Ok(Self::CantConnectToPeer(
                CantConnectToPeerRecv::decode(body)?,
            )),
            code::CANT_CREATE_ROOM => {
                Ok(Self::CantCreateRoom(read_string(&mut Cursor::new(body))?))
            }
            code::ROOM_MEMBERSHIP_GRANTED => Ok(Self::RoomMembershipGranted(read_string(
                &mut Cursor::new(body),
            )?)),
            code::ROOM_MEMBERSHIP_REVOKED => Ok(Self::RoomMembershipRevoked(read_string(
                &mut Cursor::new(body),
            )?)),
            code::ROOM_OPERATORSHIP_GRANTED => Ok(Self::RoomOperatorshipGranted(read_string(
                &mut Cursor::new(body),
            )?)),
            code::ROOM_OPERATORSHIP_REVOKED => Ok(Self::RoomOperatorshipRevoked(read_string(
                &mut Cursor::new(body),
            )?)),
            code::ENABLE_ROOM_INVITATIONS => Ok(Self::EnableRoomInvitations(
                crate::server::EnableRoomInvitations::decode(body)?,
            )),
            code::CHANGE_PASSWORD => Ok(Self::ChangePassword(
                crate::server::ChangePassword::decode(body)?,
            )),
            code::ADD_ROOM_MEMBER => Ok(Self::AddRoomMember(crate::server::AddRoomMember::decode(
                body,
            )?)),
            code::REMOVE_ROOM_MEMBER => Ok(Self::RemoveRoomMember(
                crate::server::RemoveRoomMember::decode(body)?,
            )),
            code::ADD_ROOM_OPERATOR => Ok(Self::AddRoomOperator(
                crate::server::AddRoomOperator::decode(body)?,
            )),
            code::REMOVE_ROOM_OPERATOR => Ok(Self::RemoveRoomOperator(
                crate::server::RemoveRoomOperator::decode(body)?,
            )),
            code::RESET_DISTRIBUTED => Ok(Self::ResetDistributed),
            _ => Ok(Self::Unknown {
                code,
                body: body.to_vec(),
            }),
        }
    }
}
