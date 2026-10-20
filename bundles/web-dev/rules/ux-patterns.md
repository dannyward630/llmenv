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
---

# UX Patterns

Image size, lazy loading, and resource hints: `html-baseline.md`.
Focus and announcements for dynamic changes: `htmx-accessibility.md`.

## States

- MUST design the loading, empty, and error state of every view that shows server data.
- An empty state MUST say why the view is empty and what the user can do next.
- An error state MUST say what failed and how to retry. It MUST keep the user input.

## Actions

- MUST show one clear primary action for each view.
- MUST confirm a destructive action, or offer undo. SHOULD name the object in the prompt: "Delete 3 files".
- MUST keep the user input after a failed submit. MUST NOT clear the form.
- SHOULD say what happened after an action: a saved message, a changed count, or a moved focus.

## Layout stability

- MUST reserve space for media with `width` and `height`, or with `aspect-ratio`.
- MUST reserve space for content that loads later. SHOULD use `min-height` on the container.
- MUST NOT insert content above content that the user reads.

## Perceived speed

- SHOULD use `content-visibility: auto` with `contain-intrinsic-size` for a long page section below the fold.
- SHOULD show a response in the page within 100 ms of an input. SHOULD use `hx-indicator` for a slower request.

## Budgets

Core Web Vitals at the 75th percentile MUST meet these budgets:

| Metric | Budget |
|--------|--------|
| LCP (Largest Contentful Paint) | 2.5 s or less |
| INP (Interaction to Next Paint) | 200 ms or less |
| CLS (Cumulative Layout Shift) | 0.1 or less |
