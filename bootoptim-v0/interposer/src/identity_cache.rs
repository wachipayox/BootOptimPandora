const APPCDS_IDENTITY_CACHE_ENV: &str = "BOOTOPTIM_APPCDS_IDENTITY_CACHE";
const APPCDS_IDENTITY_FORCE_STOCK_ENV: &str = "BOOTOPTIM_APPCDS_IDENTITY_FORCE_STOCK";
const APPCDS_IDENTITY_DIAGNOSTICS_ENV: &str = "BOOTOPTIM_APPCDS_IDENTITY_DIAGNOSTICS";
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

fn identity_cache_authority_reason(normal_gui: bool, requested: bool, force_stock: bool) -> &'static str {
    if !requested {
        "not-requested"
    } else if force_stock {
        "force-stock"
    } else if !cfg!(windows) {
        "unsupported-platform"
    } else if !normal_gui {
        "unknown-authority"
    } else {
        "normal-gui"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IdentityCacheRequest {
    requested: bool,
    authorized: bool,
    reason: &'static str,
}

struct IdentityPreflightDiagnostics {
    enabled: bool,
    request: IdentityCacheRequest,
    manifest: &'static str,
    eligible: u64,
    reused: u64,
    strong: u64,
    reused_bytes: u64,
    strong_bytes: u64,
    rehash_digest_same: u64,
    rehash_digest_changed: u64,
    miss_reasons: String,
    miss_examples: String,
    unverifiable: String,
    failure_details: String,
    publication: &'static str,
}

impl IdentityPreflightDiagnostics {
    fn new(request: IdentityCacheRequest) -> Self {
        let enabled = env::var_os(APPCDS_IDENTITY_DIAGNOSTICS_ENV).is_some_and(|value| value == "1");
        Self {
            enabled,
            request,
            manifest: "not-read",
            eligible: 0,
            reused: 0,
            strong: 0,
            reused_bytes: 0,
            strong_bytes: 0,
            rehash_digest_same: 0,
            rehash_digest_changed: 0,
            miss_reasons: "none".to_string(),
            miss_examples: "none".to_string(),
            unverifiable: "none".to_string(),
            failure_details: "none".to_string(),
            publication: "not-run",
        }
    }

    fn classify_manifest(&mut self, cache_dir: &Path) {
        if self.enabled {
            self.manifest = identity_manifest_state(cache_dir);
        }
    }

    fn capture_cache(&mut self, cache: &IdentityDigestCache, publication: &'static str) {
        if cache.manifest_state != "not-read" {
            self.manifest = cache.manifest_state;
        }
        self.eligible = cache.expected_keys.len() as u64;
        self.reused = cache.reused_files;
        self.strong = cache.stock_files;
        self.reused_bytes = cache.reused_bytes;
        self.strong_bytes = cache.strong_bytes;
        self.rehash_digest_same = cache.rehash_digest_same;
        self.rehash_digest_changed = cache.rehash_digest_changed;
        self.miss_reasons = cache.miss_reasons_summary();
        self.miss_examples = cache.miss_examples_summary();
        self.unverifiable = cache.unverifiable_summary();
        self.failure_details = cache.failure_details_summary();
        self.publication = publication;
    }
}

impl Drop for IdentityPreflightDiagnostics {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        eprintln!(
            "BOOTOPTIM_APPCDS_IDENTITY_DIAG requested={} authority={} authority_reason={} manifest={} eligible={} reused={} strong={} reused_bytes={} strong_bytes={} rehash_digest_same={} rehash_digest_changed={} miss_reasons={} miss_examples={} unverifiable={} failure_details={} publication={}",
            self.request.requested,
            if self.request.authorized {
                "accepted"
            } else {
                "rejected"
            },
            self.request.reason,
            self.manifest,
            self.eligible,
            self.reused,
            self.strong,
            self.reused_bytes,
            self.strong_bytes,
            self.rehash_digest_same,
            self.rehash_digest_changed,
            self.miss_reasons,
            self.miss_examples,
            self.unverifiable,
            self.failure_details,
            self.publication
        );
    }
}

#[derive(Debug)]
struct EvidenceQueryError {
    stage: &'static str,
    source: io::Error,
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

fn evidence_reuse_miss_reason(
    cached: &CachedIdentityDigest,
    current: &CurrentIdentityEvidence,
) -> Option<&'static str> {
    // Keep these predicates aligned with evidence_allows_reuse. This classifier is
    // called only by opt-in diagnostics after the unchanged fast predicate misses.
    if !current.handle_identity_unchanged {
        return Some("handle-identity-changed");
    }
    if current.volume_serial != cached.volume_serial {
        return Some("volume-serial-changed");
    }
    if current.journal_id != cached.journal_id {
        return Some("journal-id-changed");
    }
    if current.first_usn < 0
        || current.lowest_valid_usn < 0
        || current.next_usn < 0
        || current.next_usn < current.first_usn
        || current.next_usn < current.lowest_valid_usn
    {
        return Some("journal-bounds-invalid");
    }
    if current.next_usn < cached.snapshot_next_usn {
        return Some("journal-next-regressed");
    }
    if current.first_usn.max(current.lowest_valid_usn) > cached.snapshot_next_usn {
        return Some("journal-window-expired");
    }
    if current.file_id != cached.file_id {
        return Some("file-id-changed");
    }
    if current.file_usn < 0 || current.file_usn != cached.file_usn {
        return Some("file-usn-changed");
    }
    None
}

struct IdentityDigestCache {
    active: bool,
    manifest_path: PathBuf,
    cached: BTreeMap<String, CachedIdentityDigest>,
    records: BTreeMap<String, CachedIdentityDigest>,
    expected_keys: std::collections::BTreeSet<String>,
    complete: bool,
    manifest_state: &'static str,
    reused_files: u64,
    stock_files: u64,
    reused_bytes: u64,
    strong_bytes: u64,
    rehash_digest_same: u64,
    rehash_digest_changed: u64,
    miss_reasons: BTreeMap<(&'static str, &'static str), u64>,
    miss_examples: Vec<String>,
    track_reuse_diagnostics: bool,
    unverifiable: BTreeMap<&'static str, u64>,
    track_stock_diagnostics: bool,
    track_failure_diagnostics: bool,
    failure_details: Vec<String>,
    #[cfg(windows)]
    volumes: Vec<(String, u64, ntfs_usn_direct::Volume, Option<ntfs_usn_direct::JournalSnapshot>)>,
}

impl IdentityDigestCache {
    fn stock() -> Self {
        Self::stock_with_diagnostics(false)
    }

    fn stock_with_diagnostics(track_stock_diagnostics: bool) -> Self {
        Self {
            active: false,
            manifest_path: PathBuf::new(),
            cached: BTreeMap::new(),
            records: BTreeMap::new(),
            expected_keys: std::collections::BTreeSet::new(),
            complete: false,
            manifest_state: "not-read",
            reused_files: 0,
            stock_files: 0,
            reused_bytes: 0,
            strong_bytes: 0,
            rehash_digest_same: 0,
            rehash_digest_changed: 0,
            miss_reasons: BTreeMap::new(),
            miss_examples: Vec::new(),
            track_reuse_diagnostics: false,
            unverifiable: BTreeMap::new(),
            track_stock_diagnostics,
            track_failure_diagnostics: track_stock_diagnostics,
            failure_details: Vec::new(),
            #[cfg(windows)]
            volumes: Vec::new(),
        }
    }

    fn request_state(normal_gui: bool) -> IdentityCacheRequest {
        let requested = env::var_os(APPCDS_IDENTITY_CACHE_ENV).is_some_and(|value| value == "1");
        let force_stock = env::var_os(APPCDS_IDENTITY_FORCE_STOCK_ENV).is_some_and(|value| value == "1");
        IdentityCacheRequest {
            requested,
            authorized: identity_cache_authorized(normal_gui, requested, force_stock),
            reason: identity_cache_authority_reason(normal_gui, requested, force_stock),
        }
    }

    fn requested(normal_gui: bool) -> bool {
        Self::request_state(normal_gui).authorized
    }

    fn begin(cache_dir: &Path, active: bool) -> Self {
        Self::begin_with_diagnostics(cache_dir, active, false)
    }

    fn begin_with_diagnostics(cache_dir: &Path, active: bool, track_failure_diagnostics: bool) -> Self {
        let manifest_path = cache_dir.join(APPCDS_IDENTITY_MANIFEST);
        let (manifest_state, cached) = if active {
            match fs::metadata(&manifest_path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => ("absent", BTreeMap::new()),
                Err(_) => ("damaged", BTreeMap::new()),
                Ok(_) => match read_identity_manifest(&manifest_path) {
                    Some(records) => ("valid", records),
                    None => ("damaged", BTreeMap::new()),
                },
            }
        } else {
            ("not-read", BTreeMap::new())
        };
        Self {
            active,
            manifest_path,
            cached,
            records: BTreeMap::new(),
            expected_keys: std::collections::BTreeSet::new(),
            complete: active,
            manifest_state,
            reused_files: 0,
            stock_files: 0,
            reused_bytes: 0,
            strong_bytes: 0,
            rehash_digest_same: 0,
            rehash_digest_changed: 0,
            miss_reasons: BTreeMap::new(),
            miss_examples: Vec::new(),
            track_reuse_diagnostics: track_failure_diagnostics,
            unverifiable: BTreeMap::new(),
            track_stock_diagnostics: false,
            track_failure_diagnostics,
            failure_details: Vec::new(),
            #[cfg(windows)]
            volumes: Vec::new(),
        }
    }

    fn note_unverifiable(&mut self, reason: &'static str) {
        *self.unverifiable.entry(reason).or_insert(0) += 1;
    }

    fn unverifiable_summary(&self) -> String {
        if self.unverifiable.is_empty() {
            return "none".to_string();
        }
        self.unverifiable
            .iter()
            .map(|(reason, count)| format!("{reason}:{count}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn note_evidence_failure(&mut self, reason: &'static str, key: &str, error: EvidenceQueryError) {
        self.note_unverifiable(reason);
        if !self.track_failure_diagnostics || self.failure_details.len() >= 8 {
            return;
        }
        let token = sha256_hex(key.as_bytes());
        let os_error = error.source.raw_os_error().map_or_else(|| "none".to_string(), |code| code.to_string());
        self.failure_details.push(format!(
            "{reason}:stage={}:kind={:?}:os={os_error}:id={}",
            error.stage,
            error.source.kind(),
            &token[..16]
        ));
    }

    fn failure_details_summary(&self) -> String {
        if self.failure_details.is_empty() {
            "none".to_string()
        } else {
            self.failure_details.join(",")
        }
    }

    fn note_reuse_miss(&mut self, role: &'static str, reason: &'static str, key: &str, digest_same: Option<bool>) {
        if !self.track_reuse_diagnostics {
            return;
        }
        *self.miss_reasons.entry((role, reason)).or_insert(0) += 1;
        match digest_same {
            Some(true) => self.rehash_digest_same += 1,
            Some(false) => self.rehash_digest_changed += 1,
            None => {},
        }
        if self.miss_examples.len() < 16 {
            let token = sha256_hex(key.as_bytes());
            let result = match digest_same {
                Some(true) => "same",
                Some(false) => "changed",
                None => "new-or-unavailable",
            };
            self.miss_examples.push(format!("{}:{}:{}:{}", role, reason, result, &token[..16]));
        }
    }

    fn miss_reasons_summary(&self) -> String {
        if self.miss_reasons.is_empty() {
            return "none".to_string();
        }
        self.miss_reasons
            .iter()
            .map(|((role, reason), count)| format!("{role}-{reason}:{count}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn miss_examples_summary(&self) -> String {
        if self.miss_examples.is_empty() {
            "none".to_string()
        } else {
            self.miss_examples.join(",")
        }
    }

    fn resolve_raw(&mut self, role: &'static str, path: &Path) -> io::Result<(u64, String)> {
        if !self.active {
            if self.track_stock_diagnostics {
                let path_hex = encode_os(path.as_os_str()).encoded_hex;
                self.expected_keys.insert(identity_record_key(role, &path_hex));
                self.stock_files += 1;
            }
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
                    self.note_unverifiable("open");
                    let result = stock_raw_digest(path);
                    if self.track_reuse_diagnostics {
                        if let Ok((size, _)) = &result {
                            self.strong_bytes = self.strong_bytes.saturating_add(*size);
                        }
                        self.note_reuse_miss(role, "protected-open-failed", &key, None);
                    }
                    return result;
                },
            };
            let identity = protected.identity().clone();
            let evidence = match self.query_file_evidence(&protected) {
                Ok(evidence) => Some(evidence),
                Err(error) => {
                    self.note_evidence_failure("pre-evidence", &key, error);
                    None
                },
            };

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
                    if self.track_reuse_diagnostics {
                        self.reused_bytes = self.reused_bytes.saturating_add(cached.size);
                    }
                    return Ok((cached.size, cached.sha256.clone()));
                }
            }

            let (cached_digest, miss_reason) = if self.track_reuse_diagnostics {
                let cached_digest = self.cached.get(&key).map(|cached| cached.sha256.clone());
                let reason = match (self.cached.get(&key), evidence.as_ref()) {
                    (None, _) => "no-cached-record",
                    (Some(_), None) => "evidence-query-failed",
                    (Some(cached), Some(_)) if cached.role != role || cached.path_hex != path_hex => {
                        "record-shape-changed"
                    },
                    (Some(cached), Some(_)) if cached.volume_guid != identity.volume_guid => "volume-guid-changed",
                    (Some(cached), Some(_)) if cached.file_id != identity.file_id => "file-id-changed",
                    (Some(cached), Some(current)) => {
                        evidence_reuse_miss_reason(cached, current).unwrap_or("unclassified")
                    },
                };
                (cached_digest, reason)
            } else {
                (None, "diagnostics-disabled")
            };
            self.stock_files += 1;
            let (size, digest) = hash_protected_file(&mut protected)?;
            if self.track_reuse_diagnostics {
                self.strong_bytes = self.strong_bytes.saturating_add(size);
                let digest_same = cached_digest.as_deref().map(|cached| cached == digest);
                self.note_reuse_miss(role, miss_reason, &key, digest_same);
            }
            let after = match self.query_file_evidence(&protected) {
                Ok(evidence) => Some(evidence),
                Err(error) => {
                    self.note_evidence_failure("post-evidence", &key, error);
                    None
                },
            };
            if let Some(current) = after.as_ref() {
                if current.file_id == identity.file_id && current.handle_identity_unchanged {
                    self.records
                        .insert(key, record_from_current(role, path_hex, size, digest.clone(), &identity, current));
                } else {
                    self.note_unverifiable("post-identity");
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
            self.note_reuse_miss(role, "unsupported-platform", &key, None);
            self.note_unverifiable("unsupported-platform");
            stock_raw_digest(path)
        }
    }

    #[cfg(windows)]
    fn query_file_evidence(
        &mut self,
        file: &ntfs_usn_direct::ProtectedFile,
    ) -> Result<CurrentIdentityEvidence, EvidenceQueryError> {
        let identity = file.identity();
        let index = if let Some(index) = self
            .volumes
            .iter()
            .position(|(guid, serial, _, _)| guid == &identity.volume_guid && *serial == identity.volume_serial)
        {
            index
        } else {
            let volume = ntfs_usn_direct::Volume::open(identity).map_err(|source| EvidenceQueryError {
                stage: "volume-open",
                source,
            })?;
            self.volumes.push((identity.volume_guid.clone(), identity.volume_serial, volume, None));
            self.volumes.len() - 1
        };
        let previous_snapshot = self.volumes[index].3.take();
        let evidence =
            match self.volumes[index]
                .2
                .query_file_with_previous_snapshot(file, identity.file_id, previous_snapshot)
            {
                Ok(evidence) => {
                    self.volumes[index].3 = Some(evidence.journal);
                    evidence
                },
                Err(source) => {
                    self.volumes[index].3 = None;
                    return Err(EvidenceQueryError {
                        stage: "file-query",
                        source,
                    });
                },
            };
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

    fn finish(&self) -> io::Result<&'static str> {
        if !self.active {
            return Ok("not-active");
        }
        if !self.complete {
            return Ok("skipped-incomplete");
        }
        if self.records.len() != self.expected_keys.len()
            || self.expected_keys.iter().any(|key| !self.records.contains_key(key))
        {
            return Ok("skipped-coverage");
        }
        let bytes = serialize_identity_manifest(&self.records);
        #[cfg(windows)]
        {
            ntfs_usn_direct::atomic_replace(&self.manifest_path, &bytes)?;
            return Ok("published");
        }
        #[cfg(not(windows))]
        {
            let _ = bytes;
            Ok("unsupported-platform")
        }
    }

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

fn identity_manifest_state(cache_dir: &Path) -> &'static str {
    let path = cache_dir.join(APPCDS_IDENTITY_MANIFEST);
    match fs::metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => "absent",
        Err(_) => "damaged",
        Ok(_) => {
            if read_identity_manifest(&path).is_some() {
                "valid"
            } else {
                "damaged"
            }
        },
    }
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
    fn evidence_failure_diagnostic_correlates_opaque_file_id_and_os_error() {
        let raw_path_key = "classpath\0C:\\Users\\example\\private\\mod.jar";
        let expected_id = &sha256_hex(raw_path_key.as_bytes())[..16];
        let error = io::Error::from_raw_os_error(5);
        let expected_kind = format!("{:?}", error.kind());
        let mut cache = IdentityDigestCache::stock_with_diagnostics(true);
        cache.note_evidence_failure(
            "post-evidence",
            raw_path_key,
            EvidenceQueryError {
                stage: "file-query",
                source: error,
            },
        );

        let details = cache.failure_details_summary();
        assert!(details.contains(&format!("post-evidence:stage=file-query:kind={expected_kind}:os=5")));
        assert!(details.contains(&format!("id={expected_id}")));
        assert!(!details.contains("C:\\Users"));
        assert_eq!(cache.unverifiable_summary(), "post-evidence:1");
    }

    #[test]
    fn evidence_failure_details_are_not_collected_when_diagnostics_are_disabled() {
        let mut cache = IdentityDigestCache::stock();
        cache.note_evidence_failure(
            "pre-evidence",
            "classpath\0C:\\private\\file.jar",
            EvidenceQueryError {
                stage: "volume-open",
                source: io::Error::new(io::ErrorKind::Other, "not logged"),
            },
        );

        assert_eq!(cache.failure_details_summary(), "none");
        assert_eq!(cache.unverifiable_summary(), "pre-evidence:1");
    }

    #[test]
    fn reuse_miss_summary_is_opt_in_and_uses_only_opaque_tokens() {
        let mut disabled = IdentityDigestCache::stock();
        disabled.note_reuse_miss("mod", "file-usn-changed", "mod\0C:\\private\\secret.jar", Some(true));
        assert_eq!(disabled.miss_reasons_summary(), "none");
        assert_eq!(disabled.miss_examples_summary(), "none");

        let mut enabled = IdentityDigestCache::begin_with_diagnostics(Path::new("unused"), true, true);
        enabled.note_reuse_miss("mod", "file-usn-changed", "mod\0C:\\private\\secret.jar", Some(true));
        let reason = enabled.miss_reasons_summary();
        let sample = enabled.miss_examples_summary();
        assert_eq!(reason, "mod-file-usn-changed:1");
        assert!(sample.contains("mod:file-usn-changed:same:"));
        assert!(!sample.contains("C:\\private"));
        assert_eq!(enabled.rehash_digest_same, 1);
    }

    #[test]
    fn authority_is_opt_in_gui_only_and_force_stock_wins() {
        assert!(!identity_cache_authorized(false, true, false));
        assert!(!identity_cache_authorized(true, false, false));
        assert!(!identity_cache_authorized(true, true, true));
        assert_eq!(identity_cache_authority_reason(true, false, false), "not-requested");
        assert_eq!(identity_cache_authority_reason(true, true, true), "force-stock");
        if cfg!(windows) {
            assert!(identity_cache_authorized(true, true, false));
            assert_eq!(identity_cache_authority_reason(false, true, false), "unknown-authority");
            assert_eq!(identity_cache_authority_reason(true, true, false), "normal-gui");
        } else {
            assert_eq!(identity_cache_authority_reason(true, true, false), "unsupported-platform");
        }
    }

    #[test]
    fn manifest_and_publication_diagnostics_explain_absent_corrupt_and_incomplete_coverage() {
        let root = env::temp_dir().join(format!("bootoptim-id-diag-{}", unique_suffix()));
        fs::create_dir_all(&root).unwrap();

        assert_eq!(identity_manifest_state(&root), "absent");
        let mut cache = IdentityDigestCache::begin(&root, true);
        assert_eq!(cache.manifest_state, "absent");
        cache.expected_keys.insert("missing".to_string());
        assert_eq!(cache.finish().unwrap(), "skipped-coverage");

        fs::write(root.join(APPCDS_IDENTITY_MANIFEST), b"not-a-manifest\n").unwrap();
        assert_eq!(identity_manifest_state(&root), "damaged");
        let mut cache = IdentityDigestCache::begin(&root, true);
        assert_eq!(cache.manifest_state, "damaged");
        cache.complete = false;
        assert_eq!(cache.finish().unwrap(), "skipped-incomplete");

        let _ = fs::remove_dir_all(root);
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
    fn reuse_miss_diagnostics_name_the_exact_usn_guard_without_weakening_it() {
        let cached = sample_record();
        assert_eq!(evidence_reuse_miss_reason(&cached, &current()), None);

        let mut value = current();
        value.file_usn += 1;
        assert_eq!(evidence_reuse_miss_reason(&cached, &value), Some("file-usn-changed"));

        let mut value = current();
        value.journal_id += 1;
        assert_eq!(evidence_reuse_miss_reason(&cached, &value), Some("journal-id-changed"));

        let mut value = current();
        value.first_usn = 1001;
        assert_eq!(evidence_reuse_miss_reason(&cached, &value), Some("journal-window-expired"));
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
