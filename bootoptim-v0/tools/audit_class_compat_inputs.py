#!/usr/bin/env python3
"""Audit exact-pack transformation metadata without authorizing compatibility."""

from __future__ import annotations
import argparse, hashlib, io, json, re, sys, zipfile
from collections import Counter
from pathlib import Path

ROOTS = ("config", "defaultconfigs", "kubejs", "scripts")
SERVICE_MARKERS = ("ITransformationService", "ILaunchPluginService")
SUSPICIOUS = (
    "config/", "config\\", "defaultconfigs", "kubejs", "scripts",
    "options.txt", ".toml", ".properties", "resourcepacks",
    "resourcePacks", "System.getProperty", "getenv",
)
PRINTABLE = re.compile(rb"[\x20-\x7e]{4,240}")
MIXIN_NAME = re.compile(r"(?:^|/)(?:[^/]*mixins?[^/]*)\.json$", re.I)


def h(data):
    return hashlib.sha256(data).hexdigest()


def norm(name):
    return name.replace("\\", "/").lstrip("./")


def parse_manifest(data):
    text = data.decode("utf-8", "replace").replace("\r\n", "\n")
    rows = []
    for line in text.split("\n"):
        if line.startswith(" ") and rows:
            rows[-1] += line[1:]
        else:
            rows.append(line)
    out = {}
    for line in rows:
        if ": " in line:
            key, value = line.split(": ", 1)
            out[key.strip()] = value.strip()
    return out


def parse_services(data):
    out = []
    for raw in data.decode("utf-8", "replace").splitlines():
        value = raw.split("#", 1)[0].strip()
        if value:
            out.append(value)
    return out


def parse_mixin_toml(text):
    out = []
    sections = re.finditer(
        r"(?ms)^\s*\[\[mixins\]\]\s*$(.*?)(?=^\s*\[\[|\Z)", text
    )
    for section in sections:
        match = re.search(
            r'(?m)^\s*config\s*=\s*"([^"]+)"\s*(?:#.*)?$',
            section.group(1),
        )
        if match:
            out.append(match.group(1))
    return out


def entrypoint_path(name):
    return name.replace(".", "/") + ".class"


def suspicious_strings(data):
    found = set()
    for token in PRINTABLE.findall(data):
        text = token.decode("ascii", "ignore")
        if any(marker.lower() in text.lower() for marker in SUSPICIOUS):
            found.add(text)
    return sorted(found)[:80]


def audit_jar(name, data):
    result = {
        "jar": name,
        "sha256": h(data),
        "services": [],
        "mixin_configs": [],
        "mixin_plugins": [],
        "core_transform_metadata": [],
        "coremods": [],
        "entrypoint_string_hits": {},
        "parse_errors": [],
    }
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as jar:
            names = sorted(jar.namelist())
            names_set = set(names)
            dynamic = set()
            configs = set()

            for path in names:
                n = norm(path)
                if n.startswith("META-INF/services/") and not n.endswith("/"):
                    interface = n[len("META-INF/services/"):]
                    if "modlauncher" in interface.lower() or any(
                        marker in interface for marker in SERVICE_MARKERS
                    ):
                        providers = parse_services(jar.read(path))
                        result["services"].append({
                            "interface": interface,
                            "providers": providers,
                        })
                        if any(marker in interface for marker in SERVICE_MARKERS):
                            dynamic.update(providers)
                low = n.lower()
                if low.endswith("coremods.json") or low.endswith("accesstransformer.cfg"):
                    result["core_transform_metadata"].append(n)

            if "META-INF/coremods.json" in names_set:
                try:
                    parsed_coremods = json.loads(
                        jar.read("META-INF/coremods.json").decode("utf-8")
                    )
                    if not isinstance(parsed_coremods, dict):
                        raise ValueError("coremods root is not an object")
                    for core_name, script_path in sorted(parsed_coremods.items()):
                        row = {"name": core_name, "path": script_path, "hits": []}
                        if not isinstance(script_path, str) or script_path not in names_set:
                            row["status"] = "script-missing"
                            result["parse_errors"].append(
                                "coremod-script-missing:" + str(script_path)
                            )
                        else:
                            row["status"] = "scanned-script"
                            row["hits"] = suspicious_strings(jar.read(script_path))
                        result["coremods"].append(row)
                except (UnicodeDecodeError, json.JSONDecodeError, KeyError, ValueError) as exc:
                    result["parse_errors"].append(
                        "META-INF/coremods.json:" + type(exc).__name__
                    )

            if "META-INF/MANIFEST.MF" in names_set:
                attrs = parse_manifest(jar.read("META-INF/MANIFEST.MF"))
                for item in attrs.get("MixinConfigs", "").split(","):
                    item = item.strip()
                    if item:
                        configs.add(item)

            for meta in ("META-INF/neoforge.mods.toml", "META-INF/mods.toml"):
                if meta in names_set:
                    try:
                        configs.update(
                            parse_mixin_toml(jar.read(meta).decode("utf-8"))
                        )
                    except (UnicodeDecodeError, KeyError) as exc:
                        result["parse_errors"].append(
                            meta + ":" + type(exc).__name__
                        )

            for path in names:
                if MIXIN_NAME.search(norm(path)):
                    configs.add(norm(path))

            for config in sorted(configs):
                actual = config if config in names_set else config.lstrip("/")
                row = {"path": config, "plugin": None}
                if actual not in names_set:
                    row["status"] = "missing"
                    result["parse_errors"].append(
                        "mixin-config-missing:" + config
                    )
                else:
                    try:
                        parsed = json.loads(jar.read(actual).decode("utf-8"))
                        plugin = parsed.get("plugin") if isinstance(parsed, dict) else None
                        if isinstance(plugin, str) and plugin.strip():
                            plugin = plugin.strip()
                            row["plugin"] = plugin
                            result["mixin_plugins"].append({
                                "config": config,
                                "plugin": plugin,
                            })
                            dynamic.add(plugin)
                        row["status"] = "parsed"
                    except (UnicodeDecodeError, json.JSONDecodeError, KeyError) as exc:
                        row["status"] = "unparsed"
                        result["parse_errors"].append(
                            config + ":" + type(exc).__name__
                        )
                result["mixin_configs"].append(row)

            for owner in sorted(dynamic):
                path = entrypoint_path(owner)
                if path not in names_set:
                    result["entrypoint_string_hits"][owner] = {
                        "class": path,
                        "status": "class-not-in-owner-jar",
                        "hits": [],
                    }
                else:
                    result["entrypoint_string_hits"][owner] = {
                        "class": path,
                        "status": "scanned-entrypoint-only",
                        "hits": suspicious_strings(jar.read(path)),
                    }

            result["dynamic_entrypoints"] = sorted(dynamic)
            if result["parse_errors"]:
                result["audit_status"] = "unresolved-parse-error"
            elif dynamic or result["coremods"]:
                result["audit_status"] = "unresolved-dynamic-hook"
            elif result["mixin_configs"] or result["core_transform_metadata"]:
                result["audit_status"] = "static-transform-metadata-only"
            else:
                result["audit_status"] = "no-known-transform-metadata-observed"
    except zipfile.BadZipFile:
        result["parse_errors"].append("bad-jar-zip")
        result["dynamic_entrypoints"] = []
        result["audit_status"] = "unresolved-parse-error"
    return result


def root_summary(names):
    normalized = [norm(x) for x in names]
    out = {}
    for root in ROOTS:
        files = [
            x for x in normalized
            if x.startswith(root + "/") and not x.endswith("/")
        ]
        counts = Counter(Path(x).suffix.lower() or "<none>" for x in files)
        out[root] = {
            "present": any(x == root or x.startswith(root + "/") for x in normalized),
            "file_count": len(files),
            "extensions": dict(sorted(counts.items())),
        }
    out["options_txt_present"] = "options.txt" in normalized
    return out


def markdown(report):
    s = report["summary"]
    lines = [
        "# Exact-pack AppCDS transformation-input audit",
        "",
        "Fixture SHA-256: " + report["fixture_sha256"],
        "Mod JARs audited: " + str(s["mod_jar_count"]),
        "Dynamic-hook owners: " + str(s["dynamic_hook_jar_count"]),
        "Parse-error owners: " + str(s["parse_error_jar_count"]),
        "",
        "## Safety boundary",
        "",
        "This is discovery evidence, not a compatibility allowlist. A dynamic hook",
        "requires source/manual audit of external inputs. Absence of a suspicious",
        "string or known metadata hook does not prove arbitrary config/defaultconfigs/",
        "kubejs/scripts/options/resource-pack state is class-definition-neutral.",
        "",
        "Static string scanning covers only declared transformation-service, launch-",
        "plugin and Mixin-plugin entrypoint classes in their owning JAR. Reflection,",
        "delegation, generated/native code, indirect readers and non-standard loaders",
        "can evade it, so negative string evidence never authorizes READY reuse.",
        "",
        "## Transformation-relevant JARs",
        "",
        "| JAR | SHA-256 | Status | Services | Mixin plugins | String hits |",
        "| --- | --- | --- | ---: | ---: | ---: |",
    ]
    for item in report["jars"]:
        if item["audit_status"] == "no-known-transform-metadata-observed":
            continue
        hits = sum(
            len(value.get("hits", []))
            for value in item["entrypoint_string_hits"].values()
        )
        lines.append(
            "| " + item["jar"] + " | " + item["sha256"] + " | "
            + item["audit_status"] + " | " + str(len(item["services"]))
            + " | " + str(len(item["mixin_plugins"])) + " | " + str(hits) + " |"
        )
    lines.extend(["", "## Dynamic hook details", ""])
    for item in report["jars"]:
        if not item.get("dynamic_entrypoints") and not item.get("coremods") and not item.get("parse_errors"):
            continue
        lines.append("### " + item["jar"])
        lines.append("Status: " + item["audit_status"])
        if item.get("dynamic_entrypoints"):
            lines.append("Entrypoints: " + ", ".join(item["dynamic_entrypoints"]))
        if item.get("coremods"):
            for coremod in item["coremods"]:
                lines.append(
                    "Coremod " + str(coremod.get("name")) + ": "
                    + str(coremod.get("status")) + "; suspicious strings="
                    + str(len(coremod.get("hits", [])))
                )
        if item.get("parse_errors"):
            lines.append("Parse errors: " + ", ".join(item["parse_errors"]))
        for owner, evidence in item["entrypoint_string_hits"].items():
            lines.append(
                owner + ": " + evidence["status"] + "; suspicious strings="
                + str(len(evidence.get("hits", [])))
            )
            for hit in evidence.get("hits", [])[:20]:
                lines.append("  - " + hit.replace("|", "/"))
        lines.append("")
    return "\n".join(lines) + "\n"


def audit(pack):
    digest = hashlib.sha256()
    with pack.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    with zipfile.ZipFile(pack) as outer:
        names = outer.namelist()
        mods = sorted(
            x for x in names
            if re.search(r"(?:^|/)mods/[^/]+\.jar$", norm(x), re.I)
        )
        jars = [audit_jar(Path(norm(x)).name, outer.read(x)) for x in mods]
    return {
        "schema": 1,
        "fixture_sha256": digest.hexdigest(),
        "pack_roots": root_summary(names),
        "jars": jars,
        "summary": {
            "mod_jar_count": len(jars),
            "dynamic_hook_jar_count": sum(
                x.get("audit_status") == "unresolved-dynamic-hook" for x in jars
            ),
            "parse_error_jar_count": sum(
                bool(x.get("parse_errors")) for x in jars
            ),
            "static_transform_metadata_jar_count": sum(
                x.get("audit_status") == "static-transform-metadata-only"
                for x in jars
            ),
        },
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--pack-zip", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    args = parser.parse_args()
    report = audit(args.pack_zip)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    (args.output_dir / "transformation-input-audit.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    (args.output_dir / "transformation-input-audit.md").write_text(
        markdown(report), encoding="utf-8"
    )
    print(json.dumps(report["summary"], sort_keys=True))
    if report["summary"]["mod_jar_count"] == 0:
        print("no mod JARs found", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
