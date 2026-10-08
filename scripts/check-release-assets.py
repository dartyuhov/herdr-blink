#!/usr/bin/env python3
"""Require all four binary/checksum pairs before making a release public."""
import hashlib
from pathlib import Path
import sys

TARGETS = (
    "aarch64-apple-darwin", "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl", "x86_64-unknown-linux-musl",
)


def verify(directory):
    for target in TARGETS:
        binary = directory / f"herdr-blink-{target}"
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        expected = binary.with_name(binary.name + ".sha256").read_text().split()[0]
        if digest != expected:
            raise ValueError(f"Checksum mismatch: {binary.name}")


if __name__ == "__main__":
    verify(Path(sys.argv[1]))
