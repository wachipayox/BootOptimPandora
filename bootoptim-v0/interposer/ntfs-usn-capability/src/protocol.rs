//! Fixed-size metadata-only protocol from PR #11. Messages cannot carry paths or file contents.

pub const MAGIC: [u8; 8] = *b"BOPUSN01";
pub const VERSION: u16 = 1;
pub const NONCE_LEN: usize = 32;
pub const VOLUME_GUID_LEN: usize = 49;
pub const HANDSHAKE_LEN: usize = 48;
pub const REQUEST_LEN: usize = 112;
pub const RESPONSE_LEN: usize = 112;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestKind { Journal = 1, File = 2, Shutdown = 3 }
impl RequestKind { fn from_byte(v: u8) -> Option<Self> { match v { 1 => Some(Self::Journal), 2 => Some(Self::File), 3 => Some(Self::Shutdown), _ => None } } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ResponseStatus { Ok = 0, NonNtfs = 1, NotFound = 2, InvalidRequest = 3, IoError = 4 }
impl ResponseStatus { fn from_byte(v: u8) -> Option<Self> { match v { 0 => Some(Self::Ok), 1 => Some(Self::NonNtfs), 2 => Some(Self::NotFound), 3 => Some(Self::InvalidRequest), 4 => Some(Self::IoError), _ => None } } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handshake { pub nonce: [u8; NONCE_LEN], pub pid: u32 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request { pub kind: RequestKind, pub nonce: [u8; NONCE_LEN], pub volume_guid: [u8; VOLUME_GUID_LEN], pub file_id: [u8; 16] }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: ResponseStatus, pub nonce: [u8; NONCE_LEN], pub volume_serial: u64,
    pub journal_id: u64, pub first_usn: i64, pub lowest_valid_usn: i64, pub next_usn: i64,
    pub file_id: [u8; 16], pub file_usn: i64,
}
pub type JournalState = (u64, i64, i64, i64);

pub fn journal_state_is_valid(s: JournalState) -> bool { s.0 != 0 && s.1 >= 0 && s.2 >= 0 && s.3 >= s.1 && s.3 >= s.2 }
pub fn journal_transition_is_consistent(a: JournalState, b: JournalState) -> bool { journal_state_is_valid(a) && journal_state_is_valid(b) && a.0 == b.0 && b.1 >= a.1 && b.2 >= a.2 && b.3 >= a.3 }

fn header(out: &mut [u8]) { out[..8].copy_from_slice(&MAGIC); out[8..10].copy_from_slice(&VERSION.to_le_bytes()); }
fn valid(input: &[u8]) -> bool { input.len() >= 10 && input[..8] == MAGIC && u16::from_le_bytes([input[8], input[9]]) == VERSION }

pub fn encode_handshake(v: Handshake) -> [u8; HANDSHAKE_LEN] { let mut o=[0u8;HANDSHAKE_LEN]; header(&mut o); o[12..44].copy_from_slice(&v.nonce); o[44..48].copy_from_slice(&v.pid.to_le_bytes()); o }
pub fn decode_handshake(i: &[u8]) -> Option<Handshake> { if i.len()!=HANDSHAKE_LEN || !valid(i) || i[10..12] != [0,0] { return None; } let mut n=[0;NONCE_LEN]; n.copy_from_slice(&i[12..44]); Some(Handshake { nonce:n, pid:u32::from_le_bytes(i[44..48].try_into().ok()?) }) }

pub fn encode_request(v: Request) -> [u8; REQUEST_LEN] { let mut o=[0u8;REQUEST_LEN]; header(&mut o); o[10]=v.kind as u8; o[12..44].copy_from_slice(&v.nonce); o[44..93].copy_from_slice(&v.volume_guid); o[96..112].copy_from_slice(&v.file_id); o }
pub fn decode_request(i: &[u8]) -> Option<Request> { if i.len()!=REQUEST_LEN || !valid(i) || i[11]!=0 || i[93..96] != [0,0,0] { return None; } let kind=RequestKind::from_byte(i[10])?; let mut n=[0;NONCE_LEN]; n.copy_from_slice(&i[12..44]); let mut v=[0;VOLUME_GUID_LEN]; v.copy_from_slice(&i[44..93]); let mut f=[0;16]; f.copy_from_slice(&i[96..112]); Some(Request { kind, nonce:n, volume_guid:v, file_id:f }) }

pub fn encode_response(v: Response) -> [u8; RESPONSE_LEN] { let mut o=[0u8;RESPONSE_LEN]; header(&mut o); o[10]=v.status as u8; o[12..44].copy_from_slice(&v.nonce); o[44..52].copy_from_slice(&v.volume_serial.to_le_bytes()); o[52..60].copy_from_slice(&v.journal_id.to_le_bytes()); o[60..68].copy_from_slice(&v.first_usn.to_le_bytes()); o[68..76].copy_from_slice(&v.lowest_valid_usn.to_le_bytes()); o[76..84].copy_from_slice(&v.next_usn.to_le_bytes()); o[88..104].copy_from_slice(&v.file_id); o[104..112].copy_from_slice(&v.file_usn.to_le_bytes()); o }
pub fn decode_response(i: &[u8]) -> Option<Response> { if i.len()!=RESPONSE_LEN || !valid(i) || i[11]!=0 || i[84..88] != [0,0,0,0] { return None; } let status=ResponseStatus::from_byte(i[10])?; let mut n=[0;NONCE_LEN]; n.copy_from_slice(&i[12..44]); let mut f=[0;16]; f.copy_from_slice(&i[88..104]); Some(Response { status, nonce:n, volume_serial:u64::from_le_bytes(i[44..52].try_into().ok()?), journal_id:u64::from_le_bytes(i[52..60].try_into().ok()?), first_usn:i64::from_le_bytes(i[60..68].try_into().ok()?), lowest_valid_usn:i64::from_le_bytes(i[68..76].try_into().ok()?), next_usn:i64::from_le_bytes(i[76..84].try_into().ok()?), file_id:f, file_usn:i64::from_le_bytes(i[104..112].try_into().ok()?) }) }

pub fn volume_guid_bytes(value: &str) -> Option<[u8; VOLUME_GUID_LEN]> { let b=value.as_bytes(); if b.len()!=VOLUME_GUID_LEN || !value.starts_with(r"\\?\Volume{") || !value.ends_with(r"}\") { return None; } let uuid=&b[11..47]; for (i,c) in uuid.iter().copied().enumerate() { let h=matches!(i,8|13|18|23); if (h && c!=b'-') || (!h && !c.is_ascii_hexdigit()) { return None; } } let mut o=[0;VOLUME_GUID_LEN]; o.copy_from_slice(b); Some(o) }
pub fn volume_guid_str(value: &[u8; VOLUME_GUID_LEN]) -> Option<&str> { let s=std::str::from_utf8(value).ok()?; volume_guid_bytes(s)?; Some(s) }

#[cfg(test)]
mod tests { use super::*; #[test] fn fixed_round_trip() { let nonce=[7;NONCE_LEN]; let volume_guid=volume_guid_bytes(r"\\?\Volume{12345678-1234-5678-9abc-def012345678}\").unwrap(); let r=Request { kind:RequestKind::File, nonce, volume_guid, file_id:[9;16] }; assert_eq!(decode_request(&encode_request(r)),Some(r)); let mut bad=encode_request(r); bad[10]=99; assert_eq!(decode_request(&bad),None); } #[test] fn volume_is_not_path_channel(){ assert!(volume_guid_bytes(r"C:\assets\x").is_none()); } }
