# Changelog

## 0.1.1 (2026-10-08)

- Install prebuilt binaries on macOS and Linux, for ARM64 and x86_64,
  without requiring Rust or Cargo.
- Verify binary downloads with SHA-256 checksums.
- Use Rust's default musl linker to prevent Linux x86_64 startup crashes.
