# Web Stance: HTML First

The default for a web project is server-rendered HTML with progressive enhancement.
Each page MUST work without JavaScript.
JavaScript adds speed and convenience on top of a page that already works.

## Default stack

SHOULD choose the first layer that solves the problem:

1. Static HTML and CSS.
2. A static site generator or server-side includes, for shared layout across many pages.
3. Server-rendered templates from a backend, for dynamic data.
4. htmx, for partial page updates without a full reload. Rules: `htmx.md`.
5. A small amount of plain JavaScript (no build step), for one local behavior.

## Frameworks

SHOULD NOT use React, Angular, Vue, Svelte, Next.js, Nuxt, or a similar client-side framework by default.
SHOULD NOT add a bundler, a transpiler, or a hydration layer to a document site.
A blog, a docs site, a marketing site, a dashboard of forms, and a CRUD admin are documents.

A client-side framework is correct only for a true application, such as a spreadsheet, an editor, or a design tool.
Before you add one, MUST state the feature that HTML, the server, and htmx cannot deliver.
MUST get approval from the user for that choice.

## Platform before package

SHOULD use a native element or browser feature before a library:

- `<details>` and `<summary>` for an accordion.
- `<dialog>` or the `popover` attribute for a modal or a menu.
- `<input type="date">`, `type="email"`, and the other input types for pickers.
- `required`, `pattern`, `min`, and `max` for form validation. MUST validate again on the server.
- CSS nesting, custom properties, grid, `:has()`, container queries, and `clamp()` for layout and theme.

Page-level rules: `html-baseline.md`.
Accessibility: `accessibility.md`, `htmx-accessibility.md`.
Visual design and UX: `visual-design.md`, `ux-patterns.md`.
