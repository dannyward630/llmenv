---
# Umbrella overview for macOS native/build work; loads alongside the mac-dev
# deep-dives on the same file triggers instead of every session (#12).
paths:
  - "**/Info.plist"
  - "**/*.lproj/**"
  - "**/*.entitlements"
  - "**/CMakeLists.txt"
  - "**/Makefile"
  - "**/*.pbxproj"
  - "**/Package.swift"
  - "**/*.xcconfig"
---

# macOS Platform Essentials

Deep dives live in this bundle's path-scoped references:
`linking-relocatable.md`, `universal-binaries.md` (build files),
`bundle-layout.md` (Info.plist/lproj), `signing-notarization.md`
(entitlements/build files).

## Mental model

- `.app`, `.framework`, `.bundle` are plain directories with a defined
  internal structure. Structure IS the contract: the wrong layout breaks
  resource lookup, linking, and code signing.
- Every Mach-O dylib carries an **install name** (`LC_ID_DYLIB`) baked in
  at link time. Anything that links against it copies that string verbatim
  and dyld resolves it at load time — so a wrong install name is a bug you
  ship to every consumer.
- Relocatability comes from the `@rpath` indirection: dylibs say
  `@rpath/...`, executables carry `LC_RPATH` entries saying where to look.

## Hard rules

Install names and rpaths: `linking-relocatable.md`. Universal builds: `universal-binaries.md`.

1. **MUST re-sign after touching any binary.** `install_name_tool`, `lipo`,
   `strip`, resource edits inside a signed bundle — all invalidate the
   signature, and arm64 macOS kills invalidated binaries on launch.
   Minimum fix: `codesign -f -s - <path>` (ad-hoc).
2. **MUST use `Bundle`/`NSBundle`/`CFBundle` APIs for resources**, and MUST NOT use hand-built
   paths relative to the executable.
3. **SHOULD prefer `xcrun <tool>`** to locate the SDK copy of a tool when a bare
   invocation isn't found or Xcode selection matters.

## Tool cheat sheet

| Tool | Purpose | Mutates? |
|------|---------|----------|
| `otool -L` | linked dylibs + install names | no |
| `otool -l` | load commands (`LC_RPATH`, `LC_ID_DYLIB`, min OS) | no |
| `dyld_info -dependents` | modern otool alternative (macOS 13+) | no |
| `file` | arch slices at a glance | no |
| `lipo -info` / `-archs` | list slices in a (fat) binary | no |
| `lipo -create` / `-thin` / `-extract` | merge/split universal binaries | writes new file |
| `nm` | symbol table | no |
| `vtool -show` | per-slice min OS / SDK version | no (`-set-*` yes) |
| `install_name_tool` | edit install name / rpaths | **yes — re-sign after** |
| `codesign -dvvv` / `--verify --strict` | inspect / verify signature | no |
| `codesign -s` | sign (`-s -` = ad-hoc) | yes |
| `dsymutil` | extract debug symbols to .dSYM | writes .dSYM |
| `notarytool` / `stapler` | notarize / staple ticket | submits / writes |
| `spctl -a -vv` | Gatekeeper verdict | no |
| `security find-identity -v -p codesigning` | list signing identities | no |
