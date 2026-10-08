#!/bin/sh
# Install the release matching this checkout; no Rust toolchain is needed.
set -eu

fail() {
    echo "install-binary: $*" >&2
    exit 1
}

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
version=""
while IFS= read -r line; do
    case "$line" in
        'version = "'*)
            version=${line#*\"}
            version=${version%%\"*}
            break
            ;;
    esac
done < "$ROOT/herdr-plugin.toml"
case "$version" in
    ''|*[!0-9A-Za-z.+-]*) fail "invalid version in herdr-plugin.toml" ;;
esac

os=$(uname -s)
arch=$(uname -m)
case "$os:$arch" in
    Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin ;;
    Darwin:x86_64) target=x86_64-apple-darwin ;;
    Linux:aarch64|Linux:arm64) target=aarch64-unknown-linux-musl ;;
    Linux:x86_64) target=x86_64-unknown-linux-musl ;;
    *) fail "unsupported platform: $os $arch; build from source with cargo build --release --locked" ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is required to download the binary"
if command -v sha256sum >/dev/null 2>&1; then
    checksum_tool=sha256sum
elif command -v shasum >/dev/null 2>&1; then
    checksum_tool=shasum
else
    fail "sha256sum or shasum is required to verify the download"
fi

asset="herdr-blink-$target"
base="https://github.com/dartyuhov/herdr-blink/releases/download/v$version"
mkdir -p "$ROOT/target/release"
# Stage on the destination filesystem so the final rename is atomic.
tmp=$(mktemp -d "$ROOT/target/release/.install-XXXXXX")
trap 'rm -rf "$tmp"' EXIT
trap 'exit 1' HUP INT TERM

echo "Downloading blink v$version ($target)..."
for file in "$asset" "$asset.sha256"; do
    curl --fail --silent --show-error --location --retry 3 \
        --connect-timeout 15 --max-time 180 --proto '=https' --proto-redir '=https' \
        "$base/$file" -o "$tmp/$file" ||
        fail "could not download $file for v$version; check your connection and that the release is published"
done

IFS=' ' read -r expected rest < "$tmp/$asset.sha256" || fail "empty checksum file"
case "$expected" in
    *[!0-9a-fA-F]*|'') fail "invalid SHA-256 checksum" ;;
esac
[ "${#expected}" -eq 64 ] || fail "invalid SHA-256 checksum"
if [ "$checksum_tool" = sha256sum ]; then
    actual=$(sha256sum "$tmp/$asset")
else
    actual=$(shasum -a 256 "$tmp/$asset")
fi
actual=${actual%% *}
[ "$actual" = "$expected" ] || fail "checksum mismatch for $asset"

chmod 755 "$tmp/$asset"
mv -f "$tmp/$asset" "$ROOT/target/release/herdr-blink"
echo "Installed blink v$version."
