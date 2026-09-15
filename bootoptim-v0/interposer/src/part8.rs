use bootoptim_ntfs_usn_capability::{CapabilityFailure as UsnCapabilityFailure, JournalSnapshot};

const APPCDS_IDENTITY_CACHE_ENV: &str = "BOOTOPTIM_APPCDS_IDENTITY_CACHE";
const APPCDS_IDENTITY_CACHE_PROBE_ENV: &str = "BOOTOPTIM_APPCDS_IDENTITY_CACHE_PROBE";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdentityLaunchAuthority { Unknown, NormalGui }

// PR #19's interposer invocation has no stock-owned field that distinguishes
// GUI Start from --run-instance/legacy callers. An environment variable is not
// accepted as proof of launch origin. Until such authority is propagated from
// Pandora, runtime reuse stays hard closed even when the experiment is requested.
fn current_identity_launch_authority() -> IdentityLaunchAuthority { IdentityLaunchAuthority::Unknown }

#[derive(Clone, Debug, PartialEq, Eq)]
struct CachedIdentityDigest {
    role: String, path_hex: String, digest: String,
    size: u64, // diagnostic consistency only; never authorizes reuse
    journal: JournalSnapshot, file_id: [u8; 16], file_usn: i64,
}

#[derive(Clone, Debug)]
struct IdentityEvidence<'a> {
    requested: bool, authority: IdentityLaunchAuthority,
    capability: Result<(), UsnCapabilityFailure>, role: &'a str, path_hex: &'a str,
    journal: JournalSnapshot, file_id: [u8; 16], file_usn: i64,
    protected_handle_held: bool, handle_identity_unchanged: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdentityMiss { FeatureOff, SourceUnknown, Capability, RecordMismatch, Journal, FileId, FileUsn, Handle }

fn evaluate_identity_reuse(cached: &CachedIdentityDigest, evidence: &IdentityEvidence<'_>) -> Result<String, IdentityMiss> {
    if !evidence.requested { return Err(IdentityMiss::FeatureOff); }
    if evidence.authority != IdentityLaunchAuthority::NormalGui { return Err(IdentityMiss::SourceUnknown); }
    if evidence.capability.is_err() { return Err(IdentityMiss::Capability); }
    if evidence.role != cached.role || evidence.path_hex != cached.path_hex { return Err(IdentityMiss::RecordMismatch); }
    if !evidence.journal.continuous_from(cached.journal) { return Err(IdentityMiss::Journal); }
    if evidence.file_id != cached.file_id { return Err(IdentityMiss::FileId); }
    if evidence.file_usn < 0 || evidence.file_usn != cached.file_usn { return Err(IdentityMiss::FileUsn); }
    if !evidence.protected_handle_held || !evidence.handle_identity_unchanged { return Err(IdentityMiss::Handle); }
    Ok(cached.digest.clone())
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
struct IdentityProbeCounts { reused_files:u64, reused_bytes:u64, stock_files:u64, stock_bytes:u64, miss_files:u64 }

fn identity_cache_requested() -> bool { env::var_os(APPCDS_IDENTITY_CACHE_ENV).is_some_and(|value| value == "1") }
fn identity_cache_probe_requested() -> bool { env::var_os(APPCDS_IDENTITY_CACHE_PROBE_ENV).is_some_and(|value| !value.is_empty()) }

fn identity_probe_counts(inventory: ProbeInventory) -> IdentityProbeCounts {
    let stock_files = (inventory.classpath_files
        + inventory.module_path_files
        + inventory.mod_files
        + inventory.pack_input_files
        + inventory.component_files) as u64;
    let stock_bytes = inventory.classpath_bytes
        .saturating_add(inventory.module_path_bytes)
        .saturating_add(inventory.mod_bytes)
        .saturating_add(inventory.pack_input_bytes)
        .saturating_add(inventory.component_bytes);
    IdentityProbeCounts {
        reused_files: 0,
        reused_bytes: 0,
        stock_files,
        stock_bytes,
        miss_files: if identity_cache_requested() { stock_files } else { 0 },
    }
}

fn write_identity_cache_probe(counts: IdentityProbeCounts, blocked_source: bool) {
    let Some(path) = env::var_os(APPCDS_IDENTITY_CACHE_PROBE_ENV).map(PathBuf::from) else { return; };
    let text = format!(
        "{{\n  \"schema\": \"bootoptim.appcds_identity_cache_probe.v1\",\n  \"runtime_reuse_authorized\": {},\n  \"reused_files\": {},\n  \"reused_bytes\": {},\n  \"stock_files\": {},\n  \"stock_bytes\": {},\n  \"miss_files\": {}\n}}\n",
        if blocked_source { "false" } else { "true" }, counts.reused_files, counts.reused_bytes,
        counts.stock_files, counts.stock_bytes, counts.miss_files
    );
    if let Ok(mut file) = OpenOptions::new().create_new(true).write(true).open(path) { let _ = file.write_all(text.as_bytes()); }
}

fn finish_identity_cache_probe(inventory: ProbeInventory) {
    if !identity_cache_probe_requested() { return; }
    write_identity_cache_probe(
        identity_probe_counts(inventory),
        current_identity_launch_authority() != IdentityLaunchAuthority::NormalGui,
    );
}

#[cfg(test)]
mod appcds_identity_cache_tests {
    use super::*;
    const PATH:&str="63003a005c006d006f0064002e006a0061007200";
    fn cached()->CachedIdentityDigest { CachedIdentityDigest{role:"mod".into(),path_hex:PATH.into(),digest:"ab".repeat(32),size:8,journal:JournalSnapshot{volume_serial:7,journal_id:11,first_usn:10,lowest_valid_usn:20,next_usn:100},file_id:[3;16],file_usn:90} }
    fn evidence()->IdentityEvidence<'static>{IdentityEvidence{requested:true,authority:IdentityLaunchAuthority::NormalGui,capability:Ok(()),role:"mod",path_hex:PATH,journal:JournalSnapshot{volume_serial:7,journal_id:11,first_usn:10,lowest_valid_usn:20,next_usn:120},file_id:[3;16],file_usn:90,protected_handle_held:true,handle_identity_unchanged:true}}
    #[test] fn exact_evidence_is_only_reuse(){let c=cached();assert_eq!(evaluate_identity_reuse(&c,&evidence()).unwrap(),c.digest);}
    #[test] fn runtime_source_is_hard_closed_until_pandora_propagates_authority(){assert_eq!(current_identity_launch_authority(),IdentityLaunchAuthority::Unknown);let c=cached();let mut e=evidence();e.authority=current_identity_launch_authority();assert_eq!(evaluate_identity_reuse(&c,&e),Err(IdentityMiss::SourceUnknown));}
    #[test] fn same_size_mtime_style_change_is_not_authority(){let c=cached();let mut e=evidence();e.file_usn=91;assert_eq!(evaluate_identity_reuse(&c,&e),Err(IdentityMiss::FileUsn));}
    #[test] fn rename_changes_record_key(){let c=cached();let mut e=evidence();e.path_hex="00";assert_eq!(evaluate_identity_reuse(&c,&e),Err(IdentityMiss::RecordMismatch));}
    #[test] fn delete_recreate_file_id_misses(){let c=cached();let mut e=evidence();e.file_id=[4;16];assert_eq!(evaluate_identity_reuse(&c,&e),Err(IdentityMiss::FileId));}
    #[test] fn journal_restamp_regression_and_discontinuity_miss(){let c=cached();let mut e=evidence();e.journal.journal_id=12;assert_eq!(evaluate_identity_reuse(&c,&e),Err(IdentityMiss::Journal));let mut e=evidence();e.journal.next_usn=99;assert_eq!(evaluate_identity_reuse(&c,&e),Err(IdentityMiss::Journal));let mut e=evidence();e.journal.first_usn=101;e.journal.lowest_valid_usn=101;assert_eq!(evaluate_identity_reuse(&c,&e),Err(IdentityMiss::Journal));}
    #[test] fn helper_and_handle_failures_miss(){let c=cached();let mut e=evidence();e.capability=Err(UsnCapabilityFailure::UacDenied);assert_eq!(evaluate_identity_reuse(&c,&e),Err(IdentityMiss::Capability));let mut e=evidence();e.handle_identity_unchanged=false;assert_eq!(evaluate_identity_reuse(&c,&e),Err(IdentityMiss::Handle));}
    #[test] fn size_is_never_acceptance_input(){let mut c=cached();c.size=999999;assert!(evaluate_identity_reuse(&c,&evidence()).is_ok());}
    #[test] fn aggregate_counts_cover_profiled_candidate_inventory(){let inventory=ProbeInventory{classpath_files:2,classpath_bytes:20,module_path_files:1,module_path_bytes:10,mod_files:3,mod_bytes:30,pack_input_files:4,pack_input_bytes:40,component_files:2,component_bytes:50};unsafe{env::set_var(APPCDS_IDENTITY_CACHE_ENV,"1")};let counts=identity_probe_counts(inventory);unsafe{env::remove_var(APPCDS_IDENTITY_CACHE_ENV)};assert_eq!(counts.reused_files,0);assert_eq!(counts.stock_files,12);assert_eq!(counts.stock_bytes,150);assert_eq!(counts.miss_files,12);}
    #[test] fn aggregate_probe_is_create_new_and_path_free(){let p=env::temp_dir().join(format!("bootoptim-id-probe-{}",unique_suffix()));unsafe{env::set_var(APPCDS_IDENTITY_CACHE_PROBE_ENV,&p)};write_identity_cache_probe(IdentityProbeCounts{reused_files:2,reused_bytes:20,stock_files:3,stock_bytes:30,miss_files:3},true);let text=fs::read_to_string(&p).unwrap();assert!(text.contains("bootoptim.appcds_identity_cache_probe.v1"));assert!(!text.contains("mod.jar"));write_identity_cache_probe(IdentityProbeCounts{reused_files:9,..Default::default()},true);let text2=fs::read_to_string(&p).unwrap();assert_eq!(text,text2);unsafe{env::remove_var(APPCDS_IDENTITY_CACHE_PROBE_ENV)};let _=fs::remove_file(p);}
}
