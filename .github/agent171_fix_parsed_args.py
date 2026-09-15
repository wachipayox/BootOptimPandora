from pathlib import Path

files = {
    "bootoptim-v0/interposer/src/part4.rs": 2,
    "bootoptim-v0/interposer/src/part5.rs": 1,
    "bootoptim-v0/interposer/src/part7.rs": 1,
}

for name, expected in files.items():
    path = Path(name)
    text = path.read_text()
    lines = text.splitlines(keepends=True)
    out = []
    inserted = 0
    in_parsed = False
    depth = 0
    for line in lines:
        if "ParsedArgs {" in line:
            in_parsed = True
            depth = 1
        elif in_parsed:
            depth += line.count("{") - line.count("}")
        out.append(line)
        if in_parsed and "upstream_commit:" in line:
            indent = line[: len(line) - len(line.lstrip())]
            next_line = f"{indent}identity_launch_authority: IdentityLaunchAuthority::Unknown,\n"
            # Do not double-insert if a rerun sees the already-patched tree.
            if next_line not in text:
                out.append(next_line)
                inserted += 1
        if in_parsed and depth <= 0:
            in_parsed = False
    if inserted != expected:
        raise SystemExit(f"{name}: expected {expected} insertions, got {inserted}")
    path.write_text("".join(out))
