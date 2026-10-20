---
paths:
  - "**/*.entitlements"
  - "**/CMakeLists.txt"
  - "**/Makefile"
  - "**/*.pbxproj"
  - "**/Package.swift"
  - "**/*.xcconfig"
---

# Code Signing and Notarization

## Identities

- List: `security find-identity -v -p codesigning`
- **Ad-hoc** (`codesign -s -`): valid locally, satisfies the arm64
  requirement, cannot be notarized or distributed.
- **Apple Development**: local/team testing.
- **Developer ID Application**: distribution outside the App Store —
  the only identity notarization accepts.

arm64 macOS refuses to run unsigned or invalidated binaries. The linker
ad-hoc-signs at link time, but ANY later edit (`install_name_tool`,
`lipo`, `strip`) invalidates it — MUST re-sign with at least `codesign -f -s -`.

## Signing a bundle: inside-out

MUST sign nested code first, and the outer bundle last. MUST NOT use `--deep` for signing; it is deprecated: it
forces one set of options/entitlements onto every nested item and misses
non-standard locations. (Deep MAY still be used for *verification*.)

```sh
ID="Developer ID Application: Jane Dev (TEAMID)"
codesign -f --timestamp -o runtime -s "$ID" \
    MyApp.app/Contents/Frameworks/Foo.framework/Versions/A
codesign -f --timestamp -o runtime -s "$ID" \
    MyApp.app/Contents/XPCServices/Helper.xpc
codesign -f --timestamp -o runtime -s "$ID" \
    --entitlements MyApp.entitlements MyApp.app
```

- `--timestamp` (secure timestamp) and `-o runtime` (hardened runtime)
  are both REQUIRED for notarization.
- Entitlements MUST go on executables (main app, helpers), and MUST NOT go on frameworks
  or plain dylibs.

## Hardened runtime entitlements (common)

| Entitlement | When |
|-------------|------|
| `com.apple.security.cs.allow-jit` | JIT (JavaScriptCore, runtimes) |
| `com.apple.security.cs.allow-unsigned-executable-memory` | legacy JIT/codegen |
| `com.apple.security.cs.disable-library-validation` | loading third-party/ad-hoc plugins |
| `com.apple.security.automation.apple-events` | AppleScript automation |

Library validation note: hardened-runtime apps refuse dylibs signed by a
different team (or ad-hoc) unless `disable-library-validation` is set.

## Verify

```sh
codesign --verify --strict --deep -vv MyApp.app   # seal + nested code
codesign -dvvv MyApp.app                          # identity, flags, ID
spctl -a -vv MyApp.app                            # Gatekeeper verdict
```

## Notarize (Developer ID only)

```sh
xcrun notarytool store-credentials AC --apple-id you@example.com \
    --team-id TEAMID   # one-time; app-specific password
ditto -c -k --keepParent MyApp.app MyApp.zip
xcrun notarytool submit MyApp.zip --keychain-profile AC --wait
xcrun notarytool log <submission-id>   # on "Invalid": per-file reasons
xcrun stapler staple MyApp.app         # staple the ticket
```

- MUST staple the `.app` (or `.dmg`/`.pkg`) — a `.zip` cannot be stapled;
  MUST re-zip AFTER stapling for distribution.
- MUST zip with `ditto -c -k --keepParent` so symlinks/metadata survive.

## Common notarization rejections

- "not signed with a valid Developer ID certificate" — ad-hoc or
  Development identity somewhere in the bundle.
- "does not include a secure timestamp" — missing `--timestamp`.
- "hardened runtime not enabled" — missing `-o runtime` on some binary.
- Unsigned nested binary — audit every executable:
  `find MyApp.app -type f -perm +111 -exec codesign --verify {} \;`
