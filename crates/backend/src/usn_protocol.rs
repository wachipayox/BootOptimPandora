//! Fixed-size, metadata-only protocol shared by Pandora and the ephemeral USN helper.
//! No message can carry an asset path or file contents.

pub const MAGIC: [u8; 8] = *b"BOPUSN01";
pub const VERSION: u16 = 1;
pub const NONCE_LEN: usize = 32;
pub const VOLUME_GUID_LEN: usize = 49;
pub const HANDSHAKE_LEN: usize = 48;
pub const REQUEST_LEN: usize = 112;
pub const RESPONSE_LEN: usize = 112;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestKind {
    Journal = 1,
    File = 2,
    Shutdown = 3,
}

impl RequestKind {
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Journal),
            2 => Some(Self::File),
            3 => Some(Self::Shutdown),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ResponseStatus {
    Ok = 0,
    NonNtfs = 1,
    NotFound = 2,
    InvalidRequest = 3,
    IoError = 4,
}

impl ResponseStatus {
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Ok),
            1 => Some(Self::NonNtfs),
            2 => Some(Self::NotFound),
            3 => Some(Self::InvalidRequest),
            4 => Some(Self::IoError),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handshake {
    pub nonce: [u8; NONCE_LEN],
    pub pid: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub kind: RequestKind,
    pub nonce: [u8; NONCE_LEN],
    pub volume_guid: [u8; VOLUME_GUID_LEN],
    pub file_id: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: ResponseStatus,
    pub nonce: [u8; NONCE_LEN],
    pub volume_serial: u64,
    pub journal_id: u64,
    pub first_usn: i64,
    pub lowest_valid_usn: i64,
    pub next_usn: i64,
    pub file_id: [u8; 16],
    pub file_usn: i64,
}

fn put_header(out: &mut [u8]) {
    out[..8].copy_from_slice(&MAGIC);
    out[8..10].copy_from_slice(&VERSION.to_le_bytes());
}

fn valid_header(input: &[u8]) -> bool {
    input.len() >= 10 && input[..8] == MAGIC && u16::from_le_bytes([input[8], input[9]]) == VERSION
}

pub fn encode_handshake(value: Handshake) -> [u8; HANDSHAKE_LEN] {
    let mut out = [0u8; HANDSHAKE_LEN];
    put_header(&mut out);
    out[12..44].copy_from_slice(&value.nonce);
    out[44..48].copy_from_slice(&value.pid.to_le_bytes());
    out
}

pub fn decode_handshake(input: &[u8]) -> Option<Handshake> {
    if input.len() != HANDSHAKE_LEN || !valid_header(input) || input[10..12] != [0, 0] {
        return None;
    }
    let mut nonce = [0; NONCE_LEN];
    nonce.copy_from_slice(&input[12..44]);
    Some(Handshake {
        nonce,
        pid: u32::from_le_bytes(input[44..48].try_into().ok()?),
    })
}

pub fn encode_request(value: Request) -> [u8; REQUEST_LEN] {
    let mut out = [0u8; REQUEST_LEN];
    put_header(&mut out);
    out[10] = value.kind as u8;
    out[12..44].copy_from_slice(&value.nonce);
    out[44..93].copy_from_slice(&value.volume_guid);
    out[96..112].copy_from_slice(&value.file_id);
    out
}

pub fn decode_request(input: &[u8]) -> Option<Request> {
    if input.len() != REQUEST_LEN || !valid_header(input) || input[11] != 0 || input[93..96] != [0, 0, 0] {
        return None;
    }
    let kind = RequestKind::from_byte(input[10])?;
    let mut nonce = [0; NONCE_LEN];
    nonce.copy_from_slice(&input[12..44]);
    let mut volume_guid = [0; VOLUME_GUID_LEN];
    volume_guid.copy_from_slice(&input[44..93]);
    let mut file_id = [0; 16];
    file_id.copy_from_slice(&input[96..112]);
    Some(Request { kind, nonce, volume_guid, file_id })
}

pub fn encode_response(value: Response) -> [u8; RESPONSE_LEN] {
    let mut out = [0u8; RESPONSE_LEN];
    put_header(&mut out);
    out[10] = value.status as u8;
    out[12..44].copy_from_slice(&value.nonce);
    out[44..52].copy_from_slice(&value.volume_serial.to_le_bytes());
    out[52..60].copy_from_slice(&value.journal_id.to_le_bytes());
    out[60..68].copy_from_slice(&value.first_usn.to_le_bytes());
    out[68..76].copy_from_slice(&value.lowest_valid_usn.to_le_bytes());
    out[76..84].copy_from_slice(&value.next_usn.to_le_bytes());
    out[88..104].copy_from_slice(&value.file_id);
    out[104..112].copy_from_slice(&value.file_usn.to_le_bytes());
    out
}

pub fn decode_response(input: &[u8]) -> Option<Response> {
    if input.len() != RESPONSE_LEN || !valid_header(input) || input[11] != 0 || input[84..88] != [0, 0, 0, 0] {
        return None;
    }
    let status = ResponseStatus::from_byte(input[10])?;
    let mut nonce = [0; NONCE_LEN];
    nonce.copy_from_slice(&input[12..44]);
    let mut file_id = [0; 16];
    file_id.copy_from_slice(&input[88..104]);
    Some(Response {
        status,
        nonce,
        volume_serial: u64::from_le_bytes(input[44..52].try_into().ok()?),
        journal_id: u64::from_le_bytes(input[52..60].try_into().ok()?),
        first_usn: i64::from_le_bytes(input[60..68].try_into().ok()?),
        lowest_valid_usn: i64::from_le_bytes(input[68..76].try_into().ok()?),
        next_usn: i64::from_le_bytes(input[76..84].try_into().ok()?),
        file_id,
        file_usn: i64::from_le_bytes(input[104..112].try_into().ok()?),
    })
}

pub fn volume_guid_bytes(value: &str) -> Option<[u8; VOLUME_GUID_LEN]> {
    let bytes = value.as_bytes();
    if bytes.len() != VOLUME_GUID_LEN
        || !value.starts_with(r"\\?\Volume{")
        || !value.ends_with(r"}\")
    {
        return None;
    }
    let uuid = &bytes[11..47];
    for (i, byte) in uuid.iter().copied().enumerate() {
        let hyphen = matches!(i, 8 | 13 | 18 | 23);
        if (hyphen && byte != b'-') || (!hyphen && !byte.is_ascii_hexdigit()) {
            return None;
        }
    }
    let mut out = [0; VOLUME_GUID_LEN];
    out.copy_from_slice(bytes);
    Some(out)
}

pub fn volume_guid_str(value: &[u8; VOLUME_GUID_LEN]) -> Option<&str> {
    let text = std::str::from_utf8(value).ok()?;
    volume_guid_bytes(text)?;
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_is_fixed_size_and_rejects_unknowns() {
        let nonce = [7; NONCE_LEN];
        let volume_guid = volume_guid_bytes(r"\\?\Volume{12345678-1234-5678-9abc-def012345678}\").unwrap();
        let request = Request { kind: RequestKind::File, nonce, volume_guid, file_id: [9; 16] };
        let bytes = encode_request(request);
        assert_eq!(bytes.len(), REQUEST_LEN);
        assert_eq!(decode_request(&bytes), Some(request));

        let mut bad = bytes;
        bad[10] = 99;
        assert_eq!(decode_request(&bad), None);

        let response = Response {
            status: ResponseStatus::Ok,
            nonce,
            volume_serial: 1,
            journal_id: 2,
            first_usn: 3,
            lowest_valid_usn: 4,
            next_usn: 5,
            file_id: [9; 16],
            file_usn: 6,
        };
        assert_eq!(decode_response(&encode_response(response)), Some(response));
    }

    #[test]
    fn volume_guid_is_exact_not_a_path_channel() {
        assert!(volume_guid_bytes(r"\\?\Volume{12345678-1234-5678-9abc-def012345678}\").is_some());
        assert!(volume_guid_bytes(r"C:\assets\object").is_none());
        assert!(volume_guid_bytes(r"\\server\share\").is_none());
        assert!(volume_guid_bytes(r"\\?\Volume{12345678-1234-5678-9abc-def012345678}\assets").is_none());
    }
}
