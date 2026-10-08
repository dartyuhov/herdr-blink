#!/usr/bin/env python3
"""Validate a Conventional Commit title from an argument or message file."""
import argparse
from pathlib import Path
import re
import sys

PATTERN = re.compile(
    r"^(feat|fix|docs|chore|refactor|perf|test|build|ci|revert)"
    r"(?:\([a-zA-Z0-9_./-]+\))?!?: \S.*$"
)


def valid(title):
    return bool(PATTERN.fullmatch(title)) and title == title.strip()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("title", nargs="?")
    parser.add_argument("--file", type=Path)
    args = parser.parse_args()
    lines = args.file.read_text().splitlines() if args.file else []
    title = (lines[0] if lines else "") if args.file else args.title
    if not title or not valid(title):
        print("Use a Conventional Commit title, such as fix: correct agent ordering "
              "or feat(search)!: change query syntax", file=sys.stderr)
        sys.exit(1)
