---
paths:
  - "**/*.css"
  - "**/*.html"
  - "**/*.htm"
  - "**/templates/**"
---

# Visual Design

Page-level CSS defaults: `html-baseline.md`.
Accessibility rules: `accessibility.md`.

## Contrast

- Body text MUST have a 4.5:1 contrast ratio. Large text MUST have 3:1.
- A UI component, an icon that carries meaning, and a focus indicator MUST have 3:1.
- MUST NOT use color alone to carry meaning. MUST add text, an icon, or a pattern.

## User preferences

MUST honor each preference with a media query:

- `prefers-color-scheme` with `color-scheme: light dark` on `:root`.
  SHOULD use `light-dark()` where the target browsers support it.
- `prefers-reduced-motion: reduce`. MUST remove parallax, autoplay, and large movement. MAY keep a short fade.
- `prefers-contrast: more`. MUST raise the contrast of borders and text.
- `forced-colors: active`. MUST use system colors. MUST NOT hide a border that shows a boundary.
- `prefers-reduced-data: reduce`. SHOULD skip web fonts and large images where the browser reports it.

## Type and layout

- SHOULD use `rem` for font sizes, so the text follows the browser setting.
- SHOULD use fluid type with `clamp()`, for example `font-size: clamp(1rem, 0.9rem + 0.5vw, 1.25rem)`.
- SHOULD limit a text column to 60 to 75 characters: `max-width: 70ch`.
- MUST set `line-height` to at least 1.5 for body text.
- The page MUST reflow at a 320 pixel width and at 200% zoom with no horizontal scroll.
- MUST NOT set a fixed height on a text container.
- MUST NOT add `user-scalable=no` or `maximum-scale` to the viewport meta tag.

## Color blindness

A contrast ratio measures luminance only.
Red and green of equal luminance pass 4.5:1 and still look the same to a person with red-green color blindness.

- MUST mark a link inside body text with an underline, or give it 3:1 contrast against the surrounding text plus a second cue (WCAG 1.4.1).
- MUST give each status (success, warning, error) a second channel: an icon, a text label, or a pattern.
- MUST give each chart series a direct label, a marker shape, or a line style. MUST NOT rely on the legend color alone.
- SHOULD use a color-blind-safe palette, such as Okabe-Ito, for any set of colors that must stay distinct.
- MUST NOT pair red with green, or blue with purple, as the only difference between two states.

## Tokens and fonts

- MUST define colors, spacing, and font sizes as custom properties on `:root`.
- SHOULD use a system font stack first.
- When you add a web font, MUST set `font-display: swap`, and MUST preload the one critical file (`html-baseline.md`).
