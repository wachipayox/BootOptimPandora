#[cfg(windows)]
#[path = "../../ntfs_usn_direct.rs"]
mod ntfs_usn_direct;

include!("part1.rs");
include!("identity_cache.rs");
include!("part2.rs");
include!("part3.rs");
include!("part6.rs");
include!("part4.rs");
include!("part5.rs");
