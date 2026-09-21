const APPCDS_IDENTITY_CACHE_ENV: &str = "BOOTOPTIM_APPCDS_IDENTITY_CACHE";
const APPCDS_IDENTITY_FORCE_STOCK_ENV: &str = "BOOTOPTIM_APPCDS_IDENTITY_FORCE_STOCK";
const APPCDS_IDENTITY_MANIFEST: &str = "identity-manifest-v1.txt";
const APPCDS_IDENTITY_HEADER: &str = "BOOTOPTIM_APPCDS_IDENTITY_V1";

#[derive(Clone, Debug, PartialEq, Eq)]
struct CachedIdentityDigest {
    role: String,
    path_hex: String,
    size: u64,
    sha256: String,
    volume_guid: String,
    volume_serial: u64,
    journal_id: u64,
    snapshot_first_usn: i64,
    snapshot_lowest_valid_usn: i64,
    snapshot_next_usn: i64,
    file_id: [u8; 16],
    file_usn: i64,
}

#[derive(Clone, Copy, Debug)]
struct CurrentIdentityEvidence {
    volume_serial: u64,
    journal_id: u64,
    first_usn: i64,
    lowest_valid_usn: i64,
    next_usn: i64,
    file_id: [u8; 16],
    file_usn: i64,
    handle_identity_unchanged: bool,
}

fn identity_cache_authorized(normal_gui: bool, requested: bool, force_stock: bool) -> bool {
    cfg!(windows) && normal_gui && requested && !force_stock
}

fn evidence_allows_reuse(cached: &CachedIdentityDigest, current: &CurrentIdentityEvidence) -> bool {
    current.handle_identity_unchanged
        && current.volume_serial == cached.volume_serial
        && current.journal_id == cached.journal_id
        && current.first_usn >= 0
        && current.lowest_valid_usn >= 0
        && current.next_usn >= 0
        && current.file_usn >= 0
        && current.next_usn >= current.first_usn
        && current.next_usn >= current.lowest_valid_usn
        && current.next_usn >= cached.snapshot_next_usn
        && current.first_usn.max(current.lowest_valid_usn) <= cached.snapshot_next_usn
        && current.file_id == cached.file_id
        && current.file_usn == cached.file_usn
}

struct IdentityDigestCache {
    active: bool,
    manifest_path: PathBuf,
    cached: BTreeMap<String, CachedIdentityDigest>,
    records: BTreeMap<String, CachedIdentityDigest>,
    expected_keys: std::collections::BTreeSet<String>,
    complete: bool,
    reused_files: u64,
    stock_files: u64,
    #[cfg(windows)]
    volumes: Vec<(String, u64, ntfs_usn_direct::Volume)>,
}

impl IdentityDigestCache {
    fn stock() -> Self {
        Self {
            active: false,
            manifest_path: PathBuf::new(),
            cached: BTreeMap::new(),
            records: BTreeMap::new(),
            expected_keys: std::collections::BTreeSet::new(),
            complete: false,
            reused_files: 0,
            stock_files: 0,
            #[cfg(windows)]
            volumes: Vec::new(),
        }
    }

    fn requested(normal_gui: bool) -> bool {
        let requested = env::var_os(APPCDS_IDENTITY_CACHE_ENV).is_some_and(|value| value == "1");
        let force_stock = env::var_os(APPCDS_IDENTITY_FORCE_STOCK_ENV).is_some_and(|value| value == "1");
        identity_cache_authorized(normal_gui, requested, force_stock)
    }

    fn begin(cache_dir: &Path, active: bool) -> Self {
        let manifest_path = cache_dir.join(APPCDS_IDENTITY_MANIFEST);
        let cached = if active {
            read_identity_manifest(&manifest_path).unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        Self {
            active,
            manifest_path,
            cached,
            records: BTreeMap::new(),
            expected_keys: std::collections::BTreeSet::new(),
            complete: active,
            reused_files: 0,
            stock_files: 0,
            #[cfg(windows)]
            volumes: Vec::new(),
        }
    }

    fn resolve_raw(&mut self, role: &'static str, path: &Path) -> io::Result<(u64, String)> {
        if !self.active {
            return stock_raw_digest(path);
        }
        let path_hex = encode_os(path.as_os_str()).encoded_hex;
        let key = identity_record_key(role, &path_hex);
        self.expected_keys.insert(key.clone());

        #[cfg(windows)]
        {
            let mut protected = match ntfs_usn_direct::ProtectedFile::open(path) {
                Ok(file) => file,
                Err(_) => {
                    self.complete = false;
                    self.stock_files += 1;
                    return stock_raw_digest(path);
                },
            };
            let identity = protected.identity().clone();
            let evidence = self.query_file_evidence(&protected).ok();

            if let (Some(cached), Some(current)) = (self.cached.get(&key), evidence.as_ref()) {
                if cached.role == role
                    && cached.path_hex == path_hex
                    && cached.volume_guid == identity.volume_guid
                    && cached.file_id == identity.file_id
                    && evidence_allows_reuse(cached, current)
                {
                    let record =
                        record_from_current(role, path_hex, cached.size, cached.sha256.clone(), &identity, current);
                    self.records.insert(key, record);
                    self.reused_files += 1;
                    return Ok((cached.size, cached.sha256.clone()));
                }
            }

            self.stock_files += 1;
            let (size, digest) = hash_protected_file(&mut protected)?;
            let after = self.query_file_evidence(&protected).ok();
            if let Some(current) = after.as_ref() {
                if current.file_id == identity.file_id && current.handle_identity_unchanged {
                    self.records
                        .insert(key, record_from_current(role, path_hex, size, digest.clone(), &identity, current));
                } else {
                    self.complete = false;
                }
            } else {
                self.complete = false;
            }
            return Ok((size, digest));
        }

        #[cfg(not(windows))]
        {
            self.complete = false;
            self.stock_files += 1;
            stock_raw_digest(path)
        }
    }

    #[cfg(windows)]
    fn query_file_evidence(&mut self, file: &ntfs_usn_direct::ProtectedFile) -> io::Result<CurrentIdentityEvidence> {
        let identity = file.identity();
        let index = if let Some(index) = self
            .volumes
            .iter()
            .position(|(guid, serial, _)| guid == &identity.volume_guid && *serial == identity.volume_serial)
        {
            index
        } else {
            let volume = ntfs_usn_direct::Volume::open(identity)?;
            self.volumes.push((identity.volume_guid.clone(), identity.volume_serial, volume));
            self.volumes.len() - 1
        };
        let evidence = self.volumes[index].2.query_file(file, identity.file_id)?;
        Ok(CurrentIdentityEvidence {
            volume_serial: evidence.journal.volume_serial,
            journal_id: evidence.journal.journal_id,
            first_usn: evidence.journal.first_usn,
            lowest_valid_usn: evidence.journal.lowest_valid_usn,
            next_usn: evidence.journal.next_usn,
            file_id: evidence.file_id,
            file_usn: evidence.file_usn,
            handle_identity_unchanged: file.identity_unchanged(),
        })
    }

    fn finish(&self) -> io::Result<()> {
        if !self.active || !self.complete || self.records.len() != self.expected_keys.len() {
            return Ok(());
        }
        if self.expected_keys.iter().any(|key| !self.records.contains_key(key)) {
            return Ok(());
        }
        let bytes = serialize_identity_manifest(&self.records);
        #[cfg(windows)]
        {
            return ntfs_usn_direct::atomic_replace(&self.manifest_path, &bytes);
        }
        #[cfg(not(windows))]
        {
            let _ = bytes;
            Ok(())
        }
    }

    #[cfg(test)]
    fn stats(&self) -> (u64, u64) {
        (self.reused_files, self.stock_files)
    }
}

#[cfg(windows)]
fn record_from_current(
    role: &'static str,
    path_hex: String,
    size: u64,
    sha256: String,
    identity: &ntfs_usn_direct::FileIdentity,
    current: &CurrentIdentityEvidence,
) -> CachedIdentityDigest {
    CachedIdentityDigest {
        role: role.to_string(),
        path_hex,
        size,
        sha256,
        volume_guid: identity.volume_guid.clone(),
        volume_serial: current.volume_serial,
        journal_id: current.journal_id,
        snapshot_first_usn: current.first_usn,
        snapshot_lowest_valid_usn: current.lowest_valid_usn,
        snapshot_next_usn: current.next_usn,
        file_id: current.file_id,
        file_usn: current.file_usn,
    }
}

fn identity_record_key(role: &str, path_hex: &str) -> String {
    format!("{role}\0{path_hex}")
}

fn stock_raw_digest(path: &Path) -> io::Result<(u64, String)> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "identity input not file"));
    }
    Ok((meta.len(), hash_file(path)?))
}

#[cfg(windows)]
fn hash_protected_file(file: &mut ntfs_usn_direct::ProtectedFile) -> io::Result<(u64, String)> {
    use std::io::Seek;
    file.file_mut().seek(std::io::SeekFrom::Start(0))?;
    let size = file.file_mut().metadata()?.len();
    let mut state = Sha256::new();
    let mut buf = [0u8; 128 * 1024];
    loop {
        let n = file.file_mut().read(&mut buf)?;
        if n == 0 {
            break;
        }
        state.update(&buf[..n]);
    }
    Ok((size, hex(&state.finalize())))
}

fn serialize_identity_manifest(records: &BTreeMap<String, CachedIdentityDigest>) -> Vec<u8> {
    let mut out = String::new();
    out.push_str(APPCDS_IDENTITY_HEADER);
    out.push('\n');
    out.push_str(&format!("count={}\n", records.len()));
    for record in records.values() {
        out.push_str("E|");
        out.push_str(&record.role);
        out.push('|');
        out.push_str(&record.path_hex);
        out.push('|');
        out.push_str(&record.size.to_string());
        out.push('|');
        out.push_str(&record.sha256);
        out.push('|');
        out.push_str(&hex(record.volume_guid.as_bytes()));
        out.push('|');
        out.push_str(&record.volume_serial.to_string());
        out.push('|');
        out.push_str(&record.journal_id.to_string());
        out.push('|');
        out.push_str(&record.snapshot_first_usn.to_string());
        out.push('|');
        out.push_str(&record.snapshot_lowest_valid_usn.to_string());
        out.push('|');
        out.push_str(&record.snapshot_next_usn.to_string());
        out.push('|');
        out.push_str(&hex(&record.file_id));
        out.push('|');
        out.push_str(&record.file_usn.to_string());
        out.push('\n');
    }
    out.into_bytes()
}

fn read_identity_manifest(path: &Path) -> Option<BTreeMap<String, CachedIdentityDigest>> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 64 * 1024 * 1024 {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()? != APPCDS_IDENTITY_HEADER {
        return None;
    }
    let count: usize = lines.next()?.strip_prefix("count=")?.parse().ok()?;
    let mut records = BTreeMap::new();
    for line in lines {
        let fields = line.split('|').collect::<Vec<_>>();
        if fields.len() != 13 || fields[0] != "E" {
            return None;
        }
        let role = fields[1].to_string();
        if role.is_empty() || role.contains('\0') {
            return None;
        }
        let path_hex = fields[2].to_string();
        if path_hex.is_empty() || !is_lower_hex_text(&path_hex) {
            return None;
        }
        let size = fields[3].parse().ok()?;
        let sha256 = fields[4].to_string();
        if sha256.len() != 64 || !is_lower_hex_text(&sha256) {
            return None;
        }
        let volume_guid = decode_hex_utf8(fields[5])?;
        let volume_serial = fields[6].parse().ok()?;
        let journal_id = fields[7].parse().ok()?;
        let snapshot_first_usn = fields[8].parse().ok()?;
        let snapshot_lowest_valid_usn = fields[9].parse().ok()?;
        let snapshot_next_usn = fields[10].parse().ok()?;
        let file_id_vec = decode_hex(fields[11])?;
        let file_id: [u8; 16] = file_id_vec.try_into().ok()?;
        let file_usn = fields[12].parse().ok()?;
        if journal_id == 0
            || snapshot_first_usn < 0
            || snapshot_lowest_valid_usn < 0
            || snapshot_next_usn < snapshot_first_usn
            || snapshot_next_usn < snapshot_lowest_valid_usn
            || file_usn < 0
        {
            return None;
        }
        let record = CachedIdentityDigest {
            role,
            path_hex,
            size,
            sha256,
            volume_guid,
            volume_serial,
            journal_id,
            snapshot_first_usn,
            snapshot_lowest_valid_usn,
            snapshot_next_usn,
            file_id,
            file_usn,
        };
        let key = identity_record_key(&record.role, &record.path_hex);
        if records.insert(key, record).is_some() {
            return None;
        }
    }
    (records.len() == count).then_some(records)
}

fn is_lower_hex_text(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn decode_hex_utf8(value: &str) -> Option<String> {
    String::from_utf8(decode_hex(value)?).ok()
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 || !is_lower_hex_text(value) {
        return None;
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        out.push((high << 4) | low);
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

#[cfg(test)]
mod appcds_identity_cache_tests {
    use super::*;

    fn sample_record() -> CachedIdentityDigest {
        CachedIdentityDigest {
            role: "classpath".into(),
            path_hex: "006100".into(),
            size: 8,
            sha256: "11".repeat(32),
            volume_guid: r"\\?\Volume{12345678-1234-5678-9abc-def012345678}\".into(),
            volume_serial: 7,
            journal_id: 11,
            snapshot_first_usn: 50,
            snapshot_lowest_valid_usn: 100,
            snapshot_next_usn: 1000,
            file_id: [3; 16],
            file_usn: 900,
        }
    }

    fn current() -> CurrentIdentityEvidence {
        CurrentIdentityEvidence {
            volume_serial: 7,
            journal_id: 11,
            first_usn: 50,
            lowest_valid_usn: 100,
            next_usn: 1200,
            file_id: [3; 16],
            file_usn: 900,
            handle_identity_unchanged: true,
        }
    }

    #[test]
    fn authority_is_opt_in_gui_only_and_force_stock_wins() {
        assert!(!identity_cache_authorized(false, true, false));
        assert!(!identity_cache_authorized(true, false, false));
        assert!(!identity_cache_authorized(true, true, true));
        if cfg!(windows) {
            assert!(identity_cache_authorized(true, true, false));
        }
    }

    #[test]
    fn journal_restamp_regression_discontinuity_and_identity_changes_miss() {
        let cached = sample_record();
        assert!(evidence_allows_reuse(&cached, &current()));

        let mut value = current();
        value.journal_id += 1;
        assert!(!evidence_allows_reuse(&cached, &value));

        let mut value = current();
        value.next_usn = 999;
        assert!(!evidence_allows_reuse(&cached, &value));

        let mut value = current();
        value.lowest_valid_usn = 1001;
        assert!(!evidence_allows_reuse(&cached, &value));

        let mut value = current();
        value.first_usn = 1001;
        assert!(!evidence_allows_reuse(&cached, &value));

        let mut value = current();
        value.file_id[0] ^= 1;
        assert!(!evidence_allows_reuse(&cached, &value));

        let mut value = current();
        value.file_usn += 1;
        assert!(!evidence_allows_reuse(&cached, &value));

        let mut value = current();
        value.handle_identity_unchanged = false;
        assert!(!evidence_allows_reuse(&cached, &value));
    }

    #[test]
    fn manifest_round_trip_is_complete_and_partial_or_unknown_is_rejected() {
        let mut records = BTreeMap::new();
        let record = sample_record();
        records.insert(identity_record_key(&record.role, &record.path_hex), record.clone());
        let bytes = serialize_identity_manifest(&records);
        let root = env::temp_dir().join(format!("bootoptim-id-manifest-{}", unique_suffix()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("manifest");
        fs::write(&path, &bytes).unwrap();
        assert_eq!(read_identity_manifest(&path).unwrap().len(), 1);

        fs::write(&path, b"BOOTOPTIM_APPCDS_IDENTITY_V1\ncount=2\n").unwrap();
        assert!(read_identity_manifest(&path).is_none());
        fs::write(&path, b"BOOTOPTIM_APPCDS_IDENTITY_V999\ncount=0\n").unwrap();
        assert!(read_identity_manifest(&path).is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn role_and_encoded_path_are_part_of_the_key() {
        assert_ne!(identity_record_key("classpath", "00aa"), identity_record_key("module-path", "00aa"));
        assert_ne!(identity_record_key("classpath", "00aa"), identity_record_key("classpath", "00bb"));
    }
}
