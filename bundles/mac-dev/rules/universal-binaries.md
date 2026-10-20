---
paths:
  - "**/CMakeLists.txt"
  - "**/Makefile"
  - "**/*.pbxproj"
  - "**/Package.swift"
  - "**/*.xcconfig"
---

# Universal (Fat) Binaries

MUST target both `arm64` and `x86_64` for anything distributed. A universal
binary is per-arch slices in one file; MUST build each arch, then merge.

## Building per build system

- **clang directly:** `clang -arch arm64 -arch x86_64 ...` compiles and
  links both slices in one invocation (fine for simple targets).
- **CMake:** `-DCMAKE_OSX_ARCHITECTURES="arm64;x86_64"` plus
  `-DCMAKE_OSX_DEPLOYMENT_TARGET=11.0` (or per-arch floors, below).
- **Xcode:** default `ARCHS = $(ARCHS_STANDARD)` already covers both;
  MUST ensure `ONLY_ACTIVE_ARCH = NO` for Release.
- **Cargo / separate builds:** build each target, then merge:

  ```sh
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  lipo -create -output foo \
      target/aarch64-apple-darwin/release/foo \
      target/x86_64-apple-darwin/release/foo
  ```

## lipo operations

```sh
lipo -archs foo                 # e.g. "x86_64 arm64"
lipo -detailed_info foo         # offsets/sizes per slice
lipo -thin arm64 foo -output foo-arm64      # extract one arch (thin out)
lipo -extract x86_64 foo -output foo-x64    # keep fat container, one arch
lipo -create a b -output fat    # merge thin binaries
```

`lipo -create` produces an unsigned output — MUST re-sign it
(`codesign -f -s - fat` at minimum).

## Verifying

```sh
file foo                        # quick slice listing
lipo -archs foo
otool -l -arch x86_64 foo | grep -A3 LC_BUILD_VERSION   # per-slice min OS
vtool -show foo                 # min OS / SDK for every slice
arch -x86_64 ./foo              # run the Intel slice under Rosetta
```

Rosetta MUST be installed on Apple silicon test machines:
`softwareupdate --install-rosetta --agree-to-license`.

## Gotchas

- **arm64's floor is macOS 11.0.** A universal binary commonly pairs
  `x86_64-apple-macos10.15` with `arm64-apple-macos11` — MUST set deployment
  targets per arch when building slices separately; lipo happily merges
  slices with different min-OS values.
- Merge failures ("have the same architectures") mean an input was
  already fat — MUST thin it first.
- MUST test BOTH slices: an x86_64-only bug (or a missing x86_64 slice in one
  embedded dylib) surfaces only under Rosetta. Every dylib/framework in
  the bundle MUST be universal, not just the main executable —
  `find MyApp.app -type f -perm +111 -exec lipo -archs {} +` to audit.
