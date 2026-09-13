fn try_lock(path: &Path) -> io::Result<Option<LockGuard>> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        match OpenOptions::new().create(true).read(true).write(true).share_mode(0).open(path) {
            Ok(file) => Ok(Some(LockGuard { _file: file })),
            Err(e) if matches!(e.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::WouldBlock)
                || e.raw_os_error() == Some(32)
                || e.raw_os_error() == Some(33) => Ok(None),
            Err(e) => Err(e),
        }
    }
    #[cfg(not(windows))]
    {
        match OpenOptions::new().create_new(true).read(true).write(true).open(path) {
            Ok(file) => Ok(Some(LockGuard { _file: file, path: path.to_path_buf() })),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(None),
            Err(e) => Err(e),
        }
    }
}

fn write_atomic_replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", unique_suffix()));
    {
        let mut f = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn unique_suffix() -> String {
    let ns = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let count = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}-{}", std::process::id(), ns, count)
}

fn absolute_path(base: &Path, path: &Path) -> PathBuf {
    let p = if path.is_absolute() { path.to_path_buf() } else { base.join(path) };
    p.canonicalize().unwrap_or(p)
}

fn artifact_from_path(role: &'static str, path: &Path) -> io::Result<Artifact> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "artifact not file"));
    }
    Ok(Artifact {
        role,
        path: encode_os(path.as_os_str()),
        size: meta.len(),
        sha256: hash_file(path)?,
    })
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut state = Sha256::new();
    let mut buf = [0u8; 128 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 { break; }
        state.update(&buf[..n]);
    }
    Ok(hex(&state.finalize()))
}

fn find_classpath(args: &[OsString]) -> Option<OsString> {
    let mut i = 0usize;
    while i < args.len() {
        let a = &args[i];
        if a == OsStr::new("-cp") || a == OsStr::new("-classpath") || a == OsStr::new("--class-path") {
            return args.get(i + 1).cloned();
        }
        let text = a.to_string_lossy();
        if let Some(rest) = text.strip_prefix("-Djava.class.path=") {
            return Some(OsString::from(rest));
        }
        i += 1;
    }
    None
}

fn classify_arg(arg: &OsStr) -> &'static str {
    let s = arg.to_string_lossy();
    if s == "-cp" || s == "-classpath" || s == "--class-path" { "classpath-switch" }
    else if s.starts_with("-X") || s.starts_with("-XX:") { "jvm-option" }
    else if s.starts_with("-D") { "system-property" }
    else if s.starts_with("--add-opens") || s.starts_with("--add-exports") || s.starts_with("--add-reads") { "module-option" }
    else if s.starts_with('-') { "option" }
    else { "argument" }
}

fn is_sensitive_arg(arg: &OsStr) -> bool {
    let lower = arg.to_string_lossy().to_ascii_lowercase();
    ["token", "secret", "password", "passwd", "auth", "session"].iter().any(|needle| lower.contains(needle))
}

fn safe_literal(arg: &OsStr) -> Option<String> {
    let s = arg.to_string_lossy();
    if is_sensitive_arg(arg) {
        return None;
    }
    if s.starts_with("-Xms") || s.starts_with("-Xmx") || s.starts_with("-XX:") || s.starts_with("--add-") || s.starts_with("-Djava.library.path=") {
        return Some(s.into_owned());
    }
    None
}

fn has_agent_configuration(args: &[OsString]) -> bool {
    args.iter().any(|a| {
        let s = a.to_string_lossy();
        s.starts_with("-javaagent") || s.starts_with("-agentlib") || s.starts_with("-agentpath") || s == "-XX:+AllowArchivingWithJavaAgent"
    })
}

fn has_conflicting_cds_configuration(args: &[OsString]) -> bool {
    args.iter().any(|a| {
        let s = a.to_string_lossy();
        s.starts_with("-XX:SharedArchiveFile=")
            || s.starts_with("-XX:ArchiveClassesAtExit=")
            || s == "-XX:+AutoCreateSharedArchive"
            || s.starts_with("-Xshare:")
    })
}

fn has_agent_text(s: &str) -> bool {
    s.contains("-javaagent") || s.contains("-agentlib") || s.contains("-agentpath") || s.contains("-XX:+AllowArchivingWithJavaAgent")
}

fn parse_release(path: &Path) -> io::Result<BTreeMap<String, String>> {
    let text = fs::read_to_string(path)?;
    let mut out = BTreeMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            let value = v.trim().trim_matches('"').to_string();
            out.insert(k.trim().to_string(), value);
        }
    }
    Ok(out)
}

fn shared_archive_flags(state: CacheState, ready_path: &Path) -> Vec<OsString> {
    if state == CacheState::Ready {
        vec![
            OsString::from("-Xshare:auto"),
            os_flag("-XX:SharedArchiveFile=", ready_path),
        ]
    } else {
        Vec::new()
    }
}

fn os_flag(prefix: &str, path: &Path) -> OsString {
    let mut out = OsString::from(prefix);
    out.push(path.as_os_str());
    out
}

fn encode_os(value: &OsStr) -> EncodedOs {
    EncodedOs { display: value.to_string_lossy().into_owned(), encoded_hex: hex(&os_bytes(value)) }
}

fn os_bytes(value: &OsStr) -> Vec<u8> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().flat_map(u16::to_le_bytes).collect()
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().to_vec()
    }
    #[cfg(not(any(windows, unix)))]
    {
        value.to_string_lossy().as_bytes().to_vec()
    }
}

fn push_artifact_array(out: &mut String, key: &str, artifacts: &[Artifact], comma: bool) {
    out.push_str("  \""); out.push_str(key); out.push_str("\": [\n");
    for (i, a) in artifacts.iter().enumerate() {
        out.push_str("    {\"role\":"); push_json_string_value(out, a.role);
        out.push_str(",\"path\":"); push_os_value(out, &a.path);
        out.push_str(&format!(",\"size\":{},\"sha256\":", a.size)); push_json_string_value(out, &a.sha256);
        out.push('}'); if i + 1 != artifacts.len() { out.push(','); } out.push('\n');
    }
    out.push_str("  ]"); if comma { out.push(','); } out.push('\n');
}

fn push_json_num(out: &mut String, key: &str, value: u64, comma: bool) {
    out.push_str(&format!("  \"{}\": {}{}\n", key, value, if comma { "," } else { "" }));
}
fn push_json_bool(out: &mut String, key: &str, value: bool, comma: bool) {
    out.push_str(&format!("  \"{}\": {}{}\n", key, if value { "true" } else { "false" }, if comma { "," } else { "" }));
}
fn push_json_str(out: &mut String, key: &str, value: &str, comma: bool) { push_json_str_indent(out, key, value, 2, comma); }
fn push_json_str_indent(out: &mut String, key: &str, value: &str, indent: usize, comma: bool) {
    out.push_str(&" ".repeat(indent)); push_json_string_value(out, key); out.push_str(": "); push_json_string_value(out, value); if comma { out.push(','); } out.push('\n');
}
fn push_json_opt_str(out: &mut String, key: &str, value: Option<&str>, indent: usize, comma: bool) {
    out.push_str(&" ".repeat(indent)); push_json_string_value(out, key); out.push_str(": ");
    if let Some(v) = value { push_json_string_value(out, v); } else { out.push_str("null"); }
    if comma { out.push(','); } out.push('\n');
}
fn push_json_os(out: &mut String, key: &str, value: &EncodedOs, indent: usize, comma: bool) {
    out.push_str(&" ".repeat(indent)); push_json_string_value(out, key); out.push_str(": "); push_os_value(out, value); if comma { out.push(','); } out.push('\n');
}
fn push_os_value(out: &mut String, value: &EncodedOs) {
    out.push_str("{\"display\":"); push_json_string_value(out, &value.display); out.push_str(",\"encoded_hex\":"); push_json_string_value(out, &value.encoded_hex); out.push('}');
}
fn push_json_string_value(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn sha256_hex(data: &[u8]) -> String { hex(&Sha256::digest(data)) }
fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes { s.push(H[(b >> 4) as usize] as char); s.push(H[(b & 0xf) as usize] as char); }
    s
}

struct Sha256 {
    state: [u32; 8],
    len: u64,
    buffer: [u8; 64],
    used: usize,
}
impl Sha256 {
    fn new() -> Self {
        Self { state: [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19], len: 0, buffer: [0;64], used: 0 }
    }
    fn digest(data: &[u8]) -> [u8; 32] { let mut s = Self::new(); s.update(data); s.finalize() }
    fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        if self.used > 0 {
            let take = (64 - self.used).min(data.len()); self.buffer[self.used..self.used+take].copy_from_slice(&data[..take]); self.used += take; data = &data[take..];
            if self.used == 64 { let block = self.buffer; self.compress(&block); self.used = 0; }
        }
        while data.len() >= 64 { self.compress(&data[..64]); data = &data[64..]; }
        if !data.is_empty() { self.buffer[..data.len()].copy_from_slice(data); self.used = data.len(); }
    }
    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.len.wrapping_mul(8);
        self.buffer[self.used] = 0x80; self.used += 1;
        if self.used > 56 { for b in &mut self.buffer[self.used..] { *b = 0; } let block = self.buffer; self.compress(&block); self.buffer = [0;64]; self.used = 0; }
        for b in &mut self.buffer[self.used..56] { *b = 0; }
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes()); let block = self.buffer; self.compress(&block);
        let mut out = [0u8;32]; for (i,v) in self.state.iter().enumerate() { out[i*4..i*4+4].copy_from_slice(&v.to_be_bytes()); } out
    }
    fn compress(&mut self, block: &[u8]) {
        const K:[u32;64]=[0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
        let mut w=[0u32;64]; for i in 0..16 { w[i]=u32::from_be_bytes(block[i*4..i*4+4].try_into().unwrap()); } for i in 16..64 { let s0=w[i-15].rotate_right(7)^w[i-15].rotate_right(18)^(w[i-15]>>3); let s1=w[i-2].rotate_right(17)^w[i-2].rotate_right(19)^(w[i-2]>>10); w[i]=w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1); }
        let [mut a,mut b,mut c,mut d,mut e,mut f,mut g,mut h]=self.state;
        for i in 0..64 { let s1=e.rotate_right(6)^e.rotate_right(11)^e.rotate_right(25); let ch=(e&f)^((!e)&g); let t1=h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]); let s0=a.rotate_right(2)^a.rotate_right(13)^a.rotate_right(22); let maj=(a&b)^(a&c)^(b&c); let t2=s0.wrapping_add(maj); h=g; g=f; f=e; e=d.wrapping_add(t1); d=c; c=b; b=a; a=t1.wrapping_add(t2); }
        for (s,v) in self.state.iter_mut().zip([a,b,c,d,e,f,g,h]) { *s=s.wrapping_add(v); }
    }
}