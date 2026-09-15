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
mod tests {
    use super::*;

    fn volume() -> [u8; VOLUME_GUID_LEN] {
        volume_guid_bytes(r"\\?\Volume{12345678-1234-5678-9abc-def012345678}\").unwrap()
    }

    #[test]
    fn fixed_round_trip_and_reserved_bytes_are_strict() {
        let nonce=[7;NONCE_LEN];
        let request=Request { kind:RequestKind::File, nonce, volume_guid:volume(), file_id:[9;16] };
        let bytes=encode_request(request);
        assert_eq!(bytes.len(), REQUEST_LEN);
        assert_eq!(decode_request(&bytes),Some(request));
        let mut bad=bytes; bad[10]=99; assert_eq!(decode_request(&bad),None);
        let mut bad=bytes; bad[11]=1; assert_eq!(decode_request(&bad),None);
        let mut bad=bytes; bad[94]=1; assert_eq!(decode_request(&bad),None);
        assert_eq!(decode_request(&bytes[..REQUEST_LEN-1]),None);
    }

    #[test]
    fn handshake_and_response_reject_version_reserved_and_unknown_status() {
        let nonce=[3;NONCE_LEN];
        let handshake=Handshake{nonce,pid:77};
        let mut hb=encode_handshake(handshake);
        assert_eq!(decode_handshake(&hb),Some(handshake));
        hb[8]=(VERSION as u8).wrapping_add(1);
        assert_eq!(decode_handshake(&hb),None);

        let response=Response{status:ResponseStatus::Ok,nonce,volume_serial:5,journal_id:7,first_usn:10,lowest_valid_usn:20,next_usn:100,file_id:[1;16],file_usn:90};
        let bytes=encode_response(response);
        assert_eq!(decode_response(&bytes),Some(response));
        let mut bad=bytes; bad[10]=99; assert_eq!(decode_response(&bad),None);
        let mut bad=bytes; bad[85]=1; assert_eq!(decode_response(&bad),None);
    }

    #[test]
    fn journal_transition_requires_same_monotonic_valid_journal() {
        let before=(7,10,20,100);
        assert!(journal_transition_is_consistent(before,(7,11,21,101)));
        assert!(journal_transition_is_consistent(before,before));
        assert!(!journal_transition_is_consistent(before,(8,11,21,101)));
        assert!(!journal_transition_is_consistent(before,(7,9,21,101)));
        assert!(!journal_transition_is_consistent(before,(7,11,19,101)));
        assert!(!journal_transition_is_consistent(before,(7,11,21,99)));
        assert!(!journal_transition_is_consistent(before,(7,11,21,-1)));
        assert!(!journal_transition_is_consistent((0,10,20,100),before));
    }

    #[test]
    fn volume_is_not_path_channel(){
        assert!(volume_guid_bytes(r"C:\assets\x").is_none());
        assert!(volume_guid_bytes(r"\\server\share\").is_none());
        assert!(volume_guid_bytes(r"\\?\Volume{12345678-1234-5678-9abc-def012345678}\assets").is_none());
    }
}
