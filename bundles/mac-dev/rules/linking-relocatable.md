---
# Dynamic-linking deep dive; loads when build definitions are edited.
# Co-fires with build/cmake.md on CMakeLists.txt by design — different topic.
paths:
  - "**/CMakeLists.txt"
  - "**/Makefile"
  - "**/*.pbxproj"
  - "**/Package.swift"
  - "**/*.xcconfig"
---

# Relocatable Linking (install names, rpaths)

## How dyld resolves a dependency

Each load command (`LC_LOAD_DYLIB`) stores the dependency's install name
string. At load time dyld expands:

- `@executable_path` — directory of the **main executable** of the process.
- `@loader_path` — directory of the binary **containing the load command**
  (the dylib/framework doing the loading, not the main executable).
- `@rpath` — tried against each `LC_RPATH` entry, searched in order from
  the main executable and then each image up the load chain.

## Canonical layout

- Dylib install name: `@rpath/libfoo.dylib`
- Framework install name: `@rpath/Foo.framework/Versions/A/Foo`
- App main binary rpath: `@executable_path/../Frameworks`
  (frameworks/dylibs are embedded in `Contents/Frameworks/`)
- A framework's own dependency on a sibling embedded framework:
  rpath `@loader_path/..` (or `@loader_path/Frameworks` for nested copies)
- Command-line tool shipping with adjacent libs: rpath
  `@loader_path/../lib`

## Set it at build time (preferred)

SHOULD set install names and rpaths at build time.

- **CMake:** install name defaults to `@rpath` when `MACOSX_RPATH` is on
  (default). Consumers SHOULD use `set_target_properties(tool PROPERTIES
  INSTALL_RPATH "@executable_path/../Frameworks")` and
  `BUILD_WITH_INSTALL_RPATH ON` if the build-tree copy must match.
- **Xcode:** `LD_RUNPATH_SEARCH_PATHS = @executable_path/../Frameworks`;
  frameworks get `DYLIB_INSTALL_NAME_BASE = @rpath`. SHOULD use the
  "Embed Frameworks" build phase with *Code Sign on Copy*.
- **Raw clang:** `-install_name @rpath/libfoo.dylib` when linking the
  dylib; `-Wl,-rpath,@executable_path/../Frameworks` when linking the
  consumer. SHOULD add `-Wl,-headerpad_max_install_names` if paths might be
  rewritten later (install_name_tool needs the header slack).

## Fix it after the fact

```sh
install_name_tool -id "@rpath/libfoo.dylib" libfoo.dylib
install_name_tool -change /usr/local/lib/libfoo.dylib \
    "@rpath/libfoo.dylib" MyApp.app/Contents/MacOS/MyApp
install_name_tool -add_rpath "@executable_path/../Frameworks" \
    MyApp.app/Contents/MacOS/MyApp
install_name_tool -delete_rpath /old/abs/path binary
codesign -f -s - <every-binary-you-touched>   # REQUIRED on arm64
```

Typical use: vendoring a Homebrew/third-party dylib whose install name is
an absolute path — MUST copy it into `Contents/Frameworks/`, `-id` it to
`@rpath/...`, `-change` every consumer, and re-sign inside-out.

## Diagnosing

```sh
otool -L binary                     # what it links, with install names
otool -l binary | grep -A2 LC_RPATH   # rpath entries
otool -l libfoo.dylib | grep -A2 LC_ID_DYLIB
DYLD_PRINT_LIBRARIES=1 ./binary     # what actually loaded
DYLD_PRINT_SEARCHING=1 ./binary     # every path dyld tried (dyld4/macOS 12+)
```

SIP strips `DYLD_*` variables when launching protected/platform binaries —
test with your own unsigned-for-debugging build if the vars seem ignored.

## Pitfalls

- `LC_RPATH` entries are searched **in order**; a stale absolute rpath
  before the correct one can load the wrong copy silently.
- Forgetting the re-sign after `install_name_tool` → "killed: 9" on
  arm64 with no useful message. MUST re-sign, and SHOULD check `codesign --verify` first.
- `dlopen("libfoo.dylib")` with a bare leaf name searches `DYLD_*` paths
  and default locations, not your rpaths; MUST pass `@rpath/libfoo.dylib` or
  an absolute path.
- `install_name_tool` fails with "larger updated load commands do not fit"
  → MUST relink with `-headerpad_max_install_names`.
