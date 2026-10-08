#!/usr/bin/env python3
"""Regression tests for release inputs and complete publication artifacts."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import tomllib
import unittest


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


titles = load("check-commit-title")
assets = load("check-release-assets")


class ReleaseTests(unittest.TestCase):
    def test_release_versions_and_changelog_are_in_sync(self):
        root = Path(__file__).resolve().parent.parent
        package = tomllib.loads((root / "Cargo.toml").read_text())["package"]
        plugin = tomllib.loads((root / "herdr-plugin.toml").read_text())
        manifest = json.loads((root / ".release-please-manifest.json").read_text())
        lock = tomllib.loads((root / "Cargo.lock").read_text())
        locked = next(item for item in lock["package"] if item["name"] == package["name"])
        self.assertEqual(package["version"], plugin["version"])
        self.assertEqual(package["version"], manifest["."])
        self.assertEqual(package["version"], locked["version"])
        self.assertIn(package["version"], (root / "CHANGELOG.md").read_text())

    def test_conventional_titles(self):
        for title in ("fix: correct ordering", "feat(search): add filters", "feat!: change config",
                      "chore(main): release 0.1.2", "ci(release): automate publication"):
            with self.subTest(title=title):
                self.assertTrue(titles.valid(title))
        for title in ("Fix something", "feat: ", "feat:add filters", "unknown: change",
                      "fix(): change", "fix: change\nother text", " fix: change"):
            with self.subTest(title=title):
                self.assertFalse(titles.valid(title))

    def test_all_platform_assets_are_required_and_verified(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for target in assets.TARGETS:
                binary = root / f"herdr-blink-{target}"
                binary.write_bytes(b"binary fixture")
                binary.with_name(binary.name + ".sha256").write_text(
                    hashlib.sha256(binary.read_bytes()).hexdigest() + "  " + binary.name
                )
            assets.verify(root)
            binary = root / f"herdr-blink-{assets.TARGETS[0]}"
            binary.write_bytes(b"corrupt")
            with self.assertRaises(ValueError):
                assets.verify(root)
            binary.unlink()
            with self.assertRaises(FileNotFoundError):
                assets.verify(root)


if __name__ == "__main__":
    unittest.main()
