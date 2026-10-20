---
paths:
  - "**/e2e_tests/**"
  - "**/e2e/**"
  - "**/*.spec.ts"
  - "**/*.spec.js"
---

# E2E Test Rules (Playwright)

## Page Object Model (POM)

- **MUST NOT** put UI interactions or assertions directly in test files.
- All Playwright API calls MUST be in page objects.
- All methods MUST go in the appropriate page files, not test files.
- Test files MUST contain zero assertions — only verification methods on page objects.
- MUST keep a 1:1 relationship between element/locator files and page files.

## Locator Patterns

- SHOULD prefer ID-based CSS selectors (`#id_field`) over role-based selectors when strict-mode
  violations occur.
- SHOULD use a shared helper to disambiguate ambiguous text (e.g. "Create" vs "Create & Add Another")
  rather than relying on text matching.

## Verification

- When verifying against a backend (DB, API), SHOULD **retry** — flaky transaction/propagation delays are
  normal. A few attempts with a short delay handles most races.
- MUST unpack multi-value query results explicitly; MUST NOT index into tuples by position without naming.

## Accessibility checks

- MUST add `@axe-core/playwright` as the one required accessibility gate. MUST pin the version.
- MUST run `new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag22aa"]).analyze()` on each page and assert zero violations.
- MUST add a test that presses Tab through the page and asserts the focus order and the landmarks.
- SHOULD emulate `forcedColors`, `reducedMotion`, and `colorScheme` with `page.emulateMedia()`, and check the page at a 320 pixel width.
- SHOULD check color blindness through Chromium. Open `context.newCDPSession(page)`, send `Emulation.setEmulatedVisionDeficiency` with `deuteranopia`, `protanopia`, `tritanopia`, and `achromatopsia`, and review a screenshot for each. Each state and each chart series MUST stay distinguishable.
- Automation finds about one third of the problems. MUST do these by hand before a release:
  1. Use the page with the keyboard only.
  2. Use the page with VoiceOver (macOS).
  3. Check 200% zoom.
- Rules: `accessibility.md`, `htmx-accessibility.md`.

## Quick Start

```bash
cd e2e_tests
pip install -r requirements.txt   # or npm install
playwright install
pytest -vs -m regression --headed  # or: npx playwright test
```
