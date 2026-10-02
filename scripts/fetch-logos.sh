#!/bin/sh
# Downloads harness logos into assets/logos/ at build time.
#
# Logos are trademarks of their owners, so they are not committed to this
# repository. They come from the MIT-licensed LobeHub icon set
# (https://github.com/lobehub/lobe-icons), pinned to one release. A missing
# logo is not an error: blink draws a colored fallback glyph instead.
set -u

VERSION="1.97.1"
BASE="https://unpkg.com/@lobehub/icons-static-png@${VERSION}/dark"
DEST="$(dirname "$0")/../assets/logos"
mkdir -p "$DEST"

fetch() {
    name="$1"
    icon="$2"
    out="$DEST/$name.png"
    [ -s "$out" ] && return 0
    if curl -fsSL --max-time 15 "$BASE/$icon.png" -o "$out.tmp"; then
        mv "$out.tmp" "$out"
    else
        rm -f "$out.tmp"
        echo "fetch-logos: could not download $icon (fallback glyph will be used)" >&2
    fi
}

fetch claude claudecode-color
fetch codex codex-color
fetch opencode opencode
fetch pi pi
fetch copilot githubcopilot

exit 0
