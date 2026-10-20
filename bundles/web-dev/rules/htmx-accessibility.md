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
---

# htmx Accessibility

A partial update changes the page with no navigation.
The browser does not move focus, announce the change, or update the title.
Your markup MUST do these three things.
Base rules: `htmx.md`, `accessibility.md`.

## Focus

- After a swap that replaces the focused element, MUST give focus to the new content.
  MUST add `tabindex="-1"` to the new heading or region, and focus it with `hx-on::after-settle`.
- SHOULD keep focus on the same control across a swap.
  MUST give the control a stable `id`.
  htmx restores focus to an element with the same `id`.
- MUST use `hx-swap="innerHTML focus-scroll:true"` when a swap must keep the scroll position of the focused element.

## Announcements

- MUST put a status or result region in the page before the swap: `<div id="results" aria-live="polite"></div>`.
  A live region MUST exist before its content changes.
- SHOULD use `aria-live="polite"` for a result count or a saved message.
  MUST use `role="alert"` for an error only.
- MUST set `aria-busy="true"` on the target during the request.
  SHOULD use `hx-indicator` for the visible spinner, and add visually hidden text such as "Loading".

## Navigation

- With `hx-boost`, MUST return a new `<title>` in the response.
  htmx updates the document title from it.
  MUST announce the new page through a live region or move focus to the new `<h1>`.
- SHOULD use `hx-push-url="true"` for a swap that acts as a new page.
  The back button and a reload then give the same content.
- MUST follow the history rules in `htmx.md`: every pushed URL returns a full page.

## Double submit

- SHOULD add `hx-disabled-elt="this"` to a submit control, so a second click does not send a second request.
- MUST keep the control focusable. SHOULD use `aria-disabled="true"` if a disabled button loses focus in your target browsers.

## No-JavaScript fallback

MUST test every `hx-*` interaction with JavaScript off, using the keyboard only. The fallback rule is in `htmx.md`.
