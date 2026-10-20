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

# htmx Rules

Source: [htmx docs](https://htmx.org/docs/).
htmx (a small library that swaps server-rendered HTML into the page) is the default tool for partial page updates.

## Install

- MUST use the htmx 2.x line. MUST NOT use the `next` dist-tag, which is a pre-release major version.
- MUST vendor `htmx.min.js` into the static files and serve it from the same origin. MUST NOT load it from a CDN in production.
- If a CDN is necessary, MUST add `integrity` (SRI, Subresource Integrity) and `crossorigin="anonymous"`, and MUST pin the exact version.
- SHOULD add an htmx extension only for a stated need.

## Progressive enhancement

Every htmx feature MUST work as a plain link or form when JavaScript is off.

- MUST start with a real `<a href>` or `<form action method>`. Then add htmx to it.
- SHOULD prefer `hx-boost="true"` on a container. It turns its links and forms into AJAX requests, and they still work without JavaScript.
- When you use `hx-get` or `hx-post` directly, MUST put the same URL in `href` or `action`.
- SHOULD put a search input inside a `<form>` with a submit button, so Enter works without JavaScript.
- On the server, MUST read the `HX-Request` header. MUST return a fragment for an htmx request, and a full page for a plain request.
- When one URL returns both shapes, MUST send `Vary: HX-Request`. Without it, a cache can serve a fragment as a full page.

## Server responses

- MUST return HTML, not JSON. The server owns the markup.
- After a successful POST from htmx, MUST return the new HTML directly. MUST NOT redirect to a GET.
- After a successful plain POST (no `HX-Request` header), MUST redirect with 303 (POST/Redirect/GET).
- 2xx and 3xx responses swap, 4xx and 5xx do not swap by default, and 204 swaps nothing. SHOULD change this with `htmx.config.responseHandling` only for a stated need, such as a form that shows a 422 error.
- To update a second element, MUST return it with `hx-swap-oob="true"` and a matching `id`. MUST wrap a `<tr>` or `<td>` in `<template>` for an out-of-band swap.
- MAY use the `HX-Trigger`, `HX-Retarget`, `HX-Reswap`, `HX-Push-Url`, `HX-Location`, and `HX-Redirect` response headers for server-driven control.

## Attributes

- SHOULD use `hx-target` with `closest`, `next`, `previous`, `find`, or `this` before a page-wide ID.
- SHOULD choose `hx-swap` explicitly when the default `innerHTML` is wrong. `outerHTML` replaces the target element itself.
- SHOULD use `hx-trigger` modifiers to limit requests, such as `keyup changed delay:300ms` for a search box.
- SHOULD use `hx-push-url="true"` when a swap changes what the page shows. Then the back button and a reload give the same page.
- SHOULD use `hx-indicator` with the `htmx-indicator` class for request feedback.
- SHOULD use `hx-confirm` for a destructive action. A `<dialog>` is better when the text needs more than one line.
- MUST use `hx-vals` with static JSON. MUST NOT use `hx-vars` or the `js:` prefix, because both evaluate JavaScript.
- Attributes inherit to child elements. MAY set `htmx.config.disableInheritance = true` when inheritance causes a surprise, and use `hx-inherit` where you want it.

## Security

- MUST escape all user content in templates. MUST use the template engine's automatic escaping, and keep it on.
- MUST wrap any raw user HTML in an element with `hx-disable`. Injected content cannot turn it off.
- If raw user HTML is required, MUST sanitize it with an allowlist. MUST remove every `hx-*` and `data-hx-*` attribute and every `<script>`.
- MUST keep `htmx.config.selfRequestsOnly` at its default of `true`.
- SHOULD set `htmx.config.allowEval = false` and `htmx.config.allowScriptTags = false` when the page does not need them.
- MUST add `hx-history="false"` to a page with sensitive data, so htmx does not store it in `localStorage`.
- MUST send a Content-Security-Policy header with `default-src 'self'` as a base.
- MUST send the CSRF token with `hx-headers='{"X-CSRF-Token": "…"}'` on an ancestor of the requesting element. `hx-boost` does not update `<html>` or `<body>`. With `hx-boost`, MUST put the token on an element that the swap replaces.
- MUST validate every input on the server. Client validation is a convenience only.

## Configuration

- SHOULD set configuration in a `<meta name="htmx-config" content='{…}'>` tag, not in a script.
- Every URL that htmx pushes into history MUST return a full page. A user can paste it into a new tab, and htmx requests it on a history-cache miss.
- SHOULD set `"historyRestoreAsHxRequest": false`. Then a history restore does not send `HX-Request`, and the server returns the full page.
- As an alternative, MAY set `"refreshOnHistoryMiss": true` for a full browser reload on a history-cache miss.

## JavaScript with htmx

- SHOULD prefer an htmx attribute or a server response over custom JavaScript.
- SHOULD initialize a third-party library in `htmx.onLoad(fn)`, not on `DOMContentLoaded`. Then swapped content also gets the library.
- MUST call `htmx.process(el)` after your own JavaScript inserts htmx markup.
- MAY keep small event handlers next to their element with `hx-on:<event>`. This needs `allowEval`.
- To debug, MAY call `htmx.logAll()`, or load the unminified `htmx.js`.

## Forms

- SHOULD use HTML validation attributes. htmx does not send a form that fails `checkValidity()`.
- MUST add `hx-validate="true"` to a non-form element that must validate.
- MUST use `hx-encoding="multipart/form-data"` for a file upload.
