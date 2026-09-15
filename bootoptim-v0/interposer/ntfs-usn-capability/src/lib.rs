//! Capability-minimal NTFS/USN support extracted from BootOptimPandora PR #11.
//!
//! This crate deliberately contains no asset-index, repair, AppCDS, launch-mode,
//! or cache policy. It provides only authenticated metadata capability primitives.

use std::io;

pub mod protocol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityFailure {
    HelperMissing,
    HelperIdentityMismatch,
    UacDenied,
    HelperCrash,
    Timeout,
    MalformedProtocol,
    InvalidPipeAcl,
    InvalidPeerPid,
    NonNtfs,
    ReparsePoint,
    NotRegularFile,
    ProtectedHandleUnavailable,
    Io,
}

impl From<io::Error> for CapabilityFailure {
    fn from(_: io::Error) -> Self { Self::Io }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalSnapshot {
    pub volume_serial: u64,
    pub journal_id: u64,
    pub first_usn: i64,
    pub lowest_valid_usn: i64,
    pub next_usn: i64,
}

impl JournalSnapshot {
    pub fn valid(self) -> bool {
        self.journal_id != 0
            && self.first_usn >= 0
            && self.lowest_valid_usn >= 0
            && self.next_usn >= 0
            && self.next_usn >= self.first_usn
            && self.next_usn >= self.lowest_valid_usn
    }

    pub fn continuous_from(self, cached: JournalSnapshot) -> bool {
        self.valid()
            && cached.valid()
            && self.volume_serial == cached.volume_serial
            && self.journal_id == cached.journal_id
            && self.next_usn >= cached.next_usn
            && self.first_usn.max(self.lowest_valid_usn) <= cached.next_usn
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsnReply {
    pub journal: JournalSnapshot,
    pub file_id: [u8; 16],
    pub file_usn: i64,
}

pub fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 15) as usize] as char);
    }
    out
}

pub fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn decode_lower_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    if !is_lower_hex(value, N * 2) { return None; }
    let mut out = [0u8; N];
    for (idx, slot) in out.iter_mut().enumerate() {
        let hi = hex_nibble(value.as_bytes()[idx * 2])?;
        let lo = hex_nibble(value.as_bytes()[idx * 2 + 1])?;
        *slot = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{atomic_replace, open_protected, CapabilitySession, FileIdentity, ProtectedFile};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_continuity_is_fail_closed() {
        let cached = JournalSnapshot { volume_serial: 7, journal_id: 9, first_usn: 10, lowest_valid_usn: 20, next_usn: 100 };
        assert!(JournalSnapshot { next_usn: 120, ..cached }.continuous_from(cached));
        assert!(!JournalSnapshot { journal_id: 10, next_usn: 120, ..cached }.continuous_from(cached));
        assert!(!JournalSnapshot { next_usn: 99, ..cached }.continuous_from(cached));
        assert!(!JournalSnapshot { first_usn: 101, lowest_valid_usn: 101, next_usn: 120, ..cached }.continuous_from(cached));
        assert!(!JournalSnapshot { volume_serial: 8, next_usn: 120, ..cached }.continuous_from(cached));
    }

    #[test]
    fn lower_hex_parser_is_exact() {
        let bytes = [0x01u8, 0xab, 0xff];
        assert_eq!(lower_hex(&bytes), "01abff");
        assert_eq!(decode_lower_hex::<3>("01abff"), Some(bytes));
        assert!(decode_lower_hex::<3>("01ABFF").is_none());
    }
}
