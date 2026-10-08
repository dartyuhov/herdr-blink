#!/bin/sh
set -eu
root=$(git rev-parse --show-toplevel)
existing=$(git config --get core.hooksPath || true)
case "$existing" in
    ''|.githooks) ;;
    *) echo "Existing hooksPath is $existing; integrate .githooks/commit-msg there instead." >&2; exit 1 ;;
esac
chmod +x "$root/.githooks/commit-msg"
git config --local core.hooksPath .githooks
echo "Installed Conventional Commit message hook."
