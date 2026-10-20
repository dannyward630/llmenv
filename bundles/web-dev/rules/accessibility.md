---
paths:
  - "**/*.html"
  - "**/*.htm"
  - "**/*.jinja"
  - "**/*.jinja2"
  - "**/*.j2"
  - "**/*.njk"
  - "**/*.hbs"
  - "**/*.erb"
  - "**/*.ejs"
  - "**/*.liquid"
  - "**/*.gohtml"
  - "**/*.tmpl"
  - "**/*.templ"
  - "**/templates/**"
  - "**/*.css"
  - "**/*.ts"
---

# Accessibility

The target is WCAG 2.2 level AA (Web Content Accessibility Guidelines).
MUST treat it as the default.
The user MAY relax it for one stated reason.
Automated checks find about one third of the problems, so MUST do the manual checks in `e2e-playwright.md` too.
Rules for htmx swaps: `htmx-accessibility.md`.

## Structure

- MUST use the landmark elements: `<header>`, `<nav>`, `<main>`, `<footer>`.
  MUST label a landmark with `aria-label` when the page has two of the same kind.
- MUST set `lang` on `<html>`.
- MUST use one `<h1>` per page. MUST NOT skip a heading level.
- MUST put a skip link as the first focusable element: `<a href="#main">Skip to content</a>`.
  MUST give `<main>` the matching `id`.

## Native elements first

- MUST use `<a href>` for navigation and `<button>` for an action.
- SHOULD use `<details>`, `<dialog>`, and `popover` before a scripted widget.
- MUST use ARIA (Accessible Rich Internet Applications attributes) only when no native element exists.
- MUST NOT attach a click handler to a `<div>` or a `<span>`.
- MUST NOT put `role="button"` on an element that a `<button>` can replace.

## Focus and keyboard

- Every control MUST work with the keyboard alone.
- MUST show a visible `:focus-visible` style with 3:1 contrast against the background.
- MUST NOT write `outline: none` without a replacement style.
- MUST keep the DOM order equal to the visual order. MUST NOT use a positive `tabindex`.
- A sticky header MUST NOT cover the focused element. MUST set `scroll-padding-top` to the header height (WCAG 2.4.11).

## Forms

- MUST give every control a `<label>`. A placeholder is not a label.
- MUST add an `autocomplete` token to each field that collects personal data.
- MUST tie an error message to its field with `aria-describedby`, and set `aria-invalid="true"`.
- After a failed submit, MUST show an error summary that lists each error as a link to its field, and MUST move focus to the summary.
- MUST NOT rely on color alone to show an error.

## Images and media

- MUST give every `<img>` an `alt` attribute.
  MUST describe the purpose of the image.
  MUST use `alt=""` for a decorative image.
- MUST give video a caption track. MUST give audio a transcript.
- MUST NOT autoplay media with sound.

## Pointer input

- MUST make a target at least 24 by 24 CSS pixels (WCAG 2.5.8).
- MUST give every drag interaction a single-pointer alternative, such as buttons (WCAG 2.5.7).
- MUST NOT require a hover to reach content.
