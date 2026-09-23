import importlib.util
import io
import json
import tempfile
import unittest
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "audit_class_compat_inputs", HERE / "audit_class_compat_inputs.py"
)
AUDIT = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(AUDIT)


def jar_bytes(files):
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w") as jar:
        for name, data in files.items():
            jar.writestr(name, data)
    return out.getvalue()


class AuditTests(unittest.TestCase):
    def test_mixin_plugin_is_unresolved_dynamic_hook(self):
        files = {
            "example.mixins.json": json.dumps({
                "package": "example",
                "plugin": "example.ConfigPlugin",
                "mixins": ["ExampleMixin"],
            }),
            "example/ConfigPlugin.class": b"\x00config/example.toml\x00",
        }
        result = AUDIT.audit_jar("example.jar", jar_bytes(files))
        self.assertEqual(result["audit_status"], "unresolved-dynamic-hook")
        self.assertEqual(result["dynamic_entrypoints"], ["example.ConfigPlugin"])
        hits = result["entrypoint_string_hits"]["example.ConfigPlugin"]["hits"]
        self.assertTrue(any("config/example.toml" in value for value in hits))

    def test_transformation_service_is_unresolved_even_without_string_hit(self):
        files = {
            "META-INF/services/cpw.mods.modlauncher.api.ITransformationService":
                "example.TransformService\n",
            "example/TransformService.class": b"\x00nothing-interesting-here\x00",
        }
        result = AUDIT.audit_jar("service.jar", jar_bytes(files))
        self.assertEqual(result["audit_status"], "unresolved-dynamic-hook")
        self.assertEqual(result["dynamic_entrypoints"], ["example.TransformService"])
        self.assertEqual(
            result["entrypoint_string_hits"]["example.TransformService"]["hits"], []
        )

    def test_static_mixin_metadata_is_not_promoted_to_safe(self):
        files = {
            "META-INF/neoforge.mods.toml":
                '[[mixins]]\nconfig = "example.mixins.json"\n',
            "example.mixins.json": json.dumps({
                "package": "example",
                "mixins": ["ExampleMixin"],
            }),
        }
        result = AUDIT.audit_jar("static.jar", jar_bytes(files))
        self.assertEqual(result["audit_status"], "static-transform-metadata-only")
        self.assertEqual(result["dynamic_entrypoints"], [])

    def test_bad_mixin_json_fails_to_unresolved_parse_error(self):
        files = {"broken.mixins.json": "{not-json"}
        result = AUDIT.audit_jar("broken.jar", jar_bytes(files))
        self.assertEqual(result["audit_status"], "unresolved-parse-error")
        self.assertTrue(result["parse_errors"])

    def test_pack_root_inventory_does_not_classify_semantics(self):
        names = [
            "config/gameplay.toml",
            "defaultconfigs/server.toml",
            "kubejs/server_scripts/a.js",
            "scripts/example.zs",
            "options.txt",
        ]
        result = AUDIT.root_summary(names)
        self.assertEqual(result["config"]["file_count"], 1)
        self.assertEqual(result["kubejs"]["extensions"], {".js": 1})
        self.assertTrue(result["options_txt_present"])


if __name__ == "__main__":
    unittest.main()
