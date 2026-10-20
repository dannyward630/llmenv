---
paths:
  - "**/*.html"
  - "**/*.htm"
  - "**/*.css"
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

# HTML Baseline

Source: [Modern Motherfucking Website](https://modernmotherfuckingwebsite.dreamstation.systems/).
Each feature below is plain HTML or CSS.
None needs a framework, a plugin, or a build step.

## Head

Every page MUST carry this `<head>` content.
MUST fill the values for the page.

```html
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Page title</title>
<meta name="description" content="One-sentence summary.">
<link rel="canonical" href="https://example.com/page/">

<meta property="og:type" content="article">
<meta property="og:title" content="Page title">
<meta property="og:description" content="One-sentence summary.">
<meta property="og:url" content="https://example.com/page/">
<meta property="og:image" content="https://example.com/card.webp">
<meta property="og:image:alt" content="What the card image shows">
<meta name="twitter:card" content="summary_large_image">

<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "Article",
  "headline": "Page title",
  "datePublished": "2026-01-31",
  "author": { "@type": "Person", "name": "Author Name" }
}
</script>
```

- MUST put Open Graph tags in the served HTML. A crawler does not run JavaScript, so tags that a script injects are not seen.
- MUST choose the JSON-LD `@type` from schema.org, such as `Article`, `Product`, `Recipe`, `Event`, or `FAQPage`.
- MUST use absolute URLs in `og:url` and `og:image`.

## Semantic structure

- SHOULD mark a date with `<time datetime="YYYY-MM-DD">`.
- Landmarks, headings, native elements, forms, and focus: `accessibility.md`.

## Images

- MUST give every `<img>` `width` and `height` attributes. They reserve space and stop layout shift. Alt text rules: `accessibility.md`.
- SHOULD serve AVIF and WebP through `<picture>`, with a JPEG or PNG fallback in the `<img>`:

```html
<picture>
  <source type="image/avif" srcset="hero-800.avif 800w, hero-1600.avif 1600w"
          sizes="(max-width: 40rem) 100vw, 40rem">
  <source type="image/webp" srcset="hero-800.webp 800w, hero-1600.webp 1600w"
          sizes="(max-width: 40rem) 100vw, 40rem">
  <img src="hero-800.jpg" width="1600" height="800" alt="What the image shows">
</picture>
```

- SHOULD make the AVIF and WebP files one time, when you add the image: `avifenc hero.png hero.avif` and `cwebp -q 80 hero.png -o hero.webp`.
- SHOULD add `fetchpriority="high"` to the one image that matters most above the fold.
- SHOULD add `loading="lazy" decoding="async"` to each image below the fold.

## Resource hints

A page SHOULD use these hints:

- `<link rel="preconnect" href="…" crossorigin>` for a third-party origin that the page needs early.
- `<link rel="dns-prefetch" href="…">` for an origin that the page needs later, or maybe not at all.
- `<link rel="preload" href="/fonts/body.woff2" as="font" type="font/woff2" crossorigin>` for a critical file that the parser cannot discover.
- SHOULD hint only the critical files. A preload on every file gives priority to no file.

## Navigation speed

A multi-page site gets fast navigation from two additions:

```html
<script type="speculationrules">
{ "prerender": [{ "where": { "href_matches": "/*" }, "eagerness": "moderate" }] }
</script>
```

```css
@view-transition { navigation: auto; }
```

A browser without support ignores both.
SHOULD NOT add a client-side router for page speed.

## CSS

- SHOULD center the text column with `margin: 0 auto`. Most layouts need no more for mobile.
- Column width, theme tokens, type, contrast, and user-preference rules: `visual-design.md`.

## Size and dependencies

- SHOULD keep a content page small. The reference page is one HTML file of about 23 KB.
- SHOULD NOT add a cookie banner, a tracker, or a third-party script without a stated need.
- For many pages with a shared layout, SHOULD use a static site generator or server-side includes. Generate HTML, then serve HTML.
