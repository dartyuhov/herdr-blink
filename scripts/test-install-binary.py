#!/usr/bin/env python3
"""Exercise the real installer with local download fixtures and no Cargo."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


INSTALLER = Path(__file__).with_name("install-binary.sh")
REAL_BINARY = os.environ.get("BLINK_TEST_BINARY")
PAYLOAD = Path(REAL_BINARY).read_bytes() if REAL_BINARY else b"#!/bin/sh\nexit 0\n"
DOWNLOAD_STUB = r'''
import hashlib, json, os, pathlib, sys
url = sys.argv[-3]
out = pathlib.Path(sys.argv[-1])
with open(os.environ['DOWNLOAD_LOG'], 'a') as log:
    log.write(json.dumps(url) + '\n')
mode = os.environ.get('DOWNLOAD_MODE', '')
if mode == 'missing' or (mode == 'missing-checksum' and url.endswith('.sha256')):
    sys.exit(22)
payload = pathlib.Path(os.environ['TEST_PAYLOAD']).read_bytes()
if url.endswith('.sha256'):
    checksum = '0' * 64 if mode == 'corrupt' else hashlib.sha256(payload).hexdigest()
    out.write_text(checksum + '  ' + url.rsplit('/', 1)[-1][:-7] + '\n')
else:
    out.write_bytes(payload)
'''


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="blink installer ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        shutil.copyfile(INSTALLER, self.root / "scripts/install-binary.sh")
        self.manifest = self.root / "herdr-plugin.toml"
        self.manifest.write_text('id = "dartyuhov.blink"\nversion = "0.1.1"\n')
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for tool in ("dirname", "mkdir", "mktemp", "chmod", "mv", "rm"):
            location = shutil.which(tool)
            assert location, f"Missing test prerequisite: {tool}"
            (self.bin / tool).symlink_to(location)
        self.checksum_tools = [
            tool for tool in ("sha256sum", "shasum") if shutil.which(tool)
        ]
        self.assertTrue(self.checksum_tools, "A SHA-256 tool is needed for the test")
        self.use_checksum(self.checksum_tools[0])
        self.stub("curl", f"#!{sys.executable}\n" + DOWNLOAD_STUB)
        self.stub("uname", '#!/bin/sh\ncase "$1" in -s) echo "$TEST_OS";; -m) echo "$TEST_ARCH";; esac\n')
        (self.root / "payload").write_bytes(PAYLOAD)
        # Cargo is deliberately absent from PATH.
        self.env = {
            **os.environ,
            "PATH": str(self.bin),
            "DOWNLOAD_LOG": str(self.root / "downloads.jsonl"),
            "TEST_OS": "Darwin",
            "TEST_ARCH": "arm64",
            "DOWNLOAD_MODE": "",
            "TEST_PAYLOAD": str(self.root / "payload"),
        }
        self.destination = self.root / "target/release/herdr-blink"

    def stub(self, name, contents):
        path = self.bin / name
        path.write_text(contents)
        path.chmod(0o755)

    def use_checksum(self, name):
        for tool in self.checksum_tools:
            (self.bin / tool).unlink(missing_ok=True)
        location = shutil.which(name)
        assert location
        (self.bin / name).symlink_to(location)

    def run_installer(self, **env):
        return subprocess.run(
            ["/bin/sh", str(self.root / "scripts/install-binary.sh")],
            cwd="/",
            env={**self.env, **env},
            capture_output=True,
            text=True,
        )

    def assert_cleaned_up(self):
        self.assertEqual(list((self.root / "target/release").glob(".install-*")), [])

    def test_supported_platforms_without_cargo(self):
        platforms = [
            ("Darwin", "arm64", "aarch64-apple-darwin"),
            ("Darwin", "x86_64", "x86_64-apple-darwin"),
            ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
            ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
        ]
        for system, arch, target in platforms:
            with self.subTest(system=system, arch=arch):
                result = self.run_installer(TEST_OS=system, TEST_ARCH=arch)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.destination.read_bytes(), PAYLOAD)
                self.assertTrue(os.access(self.destination, os.X_OK))
                downloads = (self.root / "downloads.jsonl").read_text().splitlines()
                url = f"https://github.com/dartyuhov/herdr-blink/releases/download/v0.1.1/herdr-blink-{target}"
                self.assertEqual([json.loads(line) for line in downloads[-2:]], [url, url + ".sha256"])
                self.assert_cleaned_up()

    def test_both_checksum_tools(self):
        for tool in self.checksum_tools:
            with self.subTest(tool=tool):
                self.use_checksum(tool)
                result = self.run_installer()
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_installed_binary_runs(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run([str(self.destination)], capture_output=True, text=True)
        if REAL_BINARY:
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("usage: herdr-blink", result.stderr)
        else:
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_checksum_mismatch_preserves_existing_binary(self):
        self.destination.parent.mkdir(parents=True)
        self.destination.write_bytes(b"existing binary")
        result = self.run_installer(DOWNLOAD_MODE="corrupt")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr)
        self.assertEqual(self.destination.read_bytes(), b"existing binary")
        self.assert_cleaned_up()

    def test_missing_release_or_checksum_does_not_install(self):
        for mode in ("missing", "missing-checksum"):
            with self.subTest(mode=mode):
                result = self.run_installer(DOWNLOAD_MODE=mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("release is published", result.stderr)
                self.assertFalse(self.destination.exists())
                self.assert_cleaned_up()

    def test_unsupported_platform(self):
        result = self.run_installer(TEST_ARCH="riscv64", TEST_OS="Linux")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsupported platform", result.stderr)
        self.assertFalse((self.root / "downloads.jsonl").exists())

    def test_version_is_read_from_checkout(self):
        self.manifest.write_text('version = "0.2.0"\n')
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("/v0.2.0/", (self.root / "downloads.jsonl").read_text())

    def test_missing_checksum_tool(self):
        for tool in self.checksum_tools:
            (self.bin / tool).unlink(missing_ok=True)
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("required to verify", result.stderr)
        self.assertFalse((self.root / "downloads.jsonl").exists())

    def test_invalid_version(self):
        self.manifest.write_text('version = "../latest"\n')
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("invalid version", result.stderr)


if __name__ == "__main__":
    unittest.main()
