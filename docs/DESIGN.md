---
version: alpha
name: CortexFS
description: >
  Visual identity for CortexFS — a FUSE Agent OS. Warm paper surfaces, deep ink
  type, mint interaction accent, and monospace technical instrumentation.
  Applied to docs-site, marketing surfaces, and agent-facing HTML demos.
colors:
  primary: "#111312"
  secondary: "#66716c"
  tertiary: "#2A8F73"
  neutral: "#F7F5F1"
  surface: "#FFFDFA"
  surface-panel: "#FFFFFF"
  surface-coal: "#181B19"
  on-primary: "#FFFDFA"
  on-coal: "#FFFDFA"
  on-coal-muted: "rgba(255, 253, 250, 0.68)"
  line: "#DED8CA"
  soft: "#8A948F"
  amber: "#D28A28"
  rose: "#B65348"
  blue: "#3F6FA7"
  signal: "#D8FF66"
  ink-hover: "#000000"
typography:
  display-lg:
    fontFamily: Georgia, "Times New Roman", ui-serif, serif
    fontSize: 72px
    fontWeight: 600
    lineHeight: 1.12
    letterSpacing: 0em
  headline-lg:
    fontFamily: Georgia, "Times New Roman", ui-serif, serif
    fontSize: 58px
    fontWeight: 600
    lineHeight: 0.98
    letterSpacing: 0em
  headline-md:
    fontFamily: Georgia, "Times New Roman", ui-serif, serif
    fontSize: 32px
    fontWeight: 600
    lineHeight: 1.1
    letterSpacing: 0em
  title-md:
    fontFamily: Georgia, "Times New Roman", ui-serif, serif
    fontSize: 22px
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: 0em
  body-md:
    fontFamily: Inter, ui-sans-serif, system-ui, sans-serif
    fontSize: 15px
    fontWeight: 400
    lineHeight: 1.65
  body-sm:
    fontFamily: Inter, ui-sans-serif, system-ui, sans-serif
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.68
  label-caps:
    fontFamily: Inter, ui-sans-serif, system-ui, sans-serif
    fontSize: 12px
    fontWeight: 900
    lineHeight: 1
    letterSpacing: 0.12em
  label-md:
    fontFamily: Inter, ui-sans-serif, system-ui, sans-serif
    fontSize: 14px
    fontWeight: 750
    lineHeight: 1.2
  mono-md:
    fontFamily: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace
    fontSize: 13px
    fontWeight: 800
    lineHeight: 1.52
  mono-sm:
    fontFamily: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace
    fontSize: 12px
    fontWeight: 850
    lineHeight: 1.4
  brand-mark:
    fontFamily: Georgia, "Times New Roman", ui-serif, serif
    fontSize: 56px
    fontWeight: 700
    lineHeight: 1
    letterSpacing: 0em
rounded:
  sm: 6px
  md: 8px
  lg: 12px
  full: 999px
spacing:
  xs: 4px
  sm: 8px
  md: 16px
  lg: 24px
  xl: 32px
  "2xl": 48px
  "3xl": 72px
  gutter: 54px
  margin: 34px
  band: 72px
  content-max: 1320px
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-primary}"
    rounded: "{rounded.md}"
    padding: 10px
    height: 42px
    typography: "{typography.label-md}"
  button-primary-hover:
    backgroundColor: "{colors.ink-hover}"
    textColor: "#FFFFFF"
  button-secondary:
    backgroundColor: "{colors.surface-panel}"
    textColor: "{colors.primary}"
    rounded: "{rounded.md}"
    padding: 10px
    height: 42px
    typography: "{typography.label-md}"
  button-secondary-hover:
    backgroundColor: "{colors.surface-panel}"
    textColor: "{colors.primary}"
  chip-proof:
    backgroundColor: "transparent"
    textColor: "{colors.soft}"
    rounded: "{rounded.md}"
    padding: 8px
    typography: "{typography.mono-sm}"
  card-feature:
    backgroundColor: "{colors.surface-panel}"
    textColor: "{colors.primary}"
    rounded: "{rounded.md}"
    padding: 18px
  card-step:
    backgroundColor: "{colors.surface-panel}"
    textColor: "{colors.primary}"
    rounded: "{rounded.md}"
    padding: 20px
  terminal-frame:
    backgroundColor: "{colors.surface-coal}"
    textColor: "{colors.on-coal}"
    rounded: "{rounded.md}"
    padding: 24px
  navbar-link:
    backgroundColor: "transparent"
    textColor: "{colors.secondary}"
    rounded: "{rounded.full}"
    padding: 8px
    typography: "{typography.label-md}"
  navbar-link-active:
    backgroundColor: "color-mix(in srgb, #111312 8%, transparent)"
    textColor: "{colors.primary}"
  code-block:
    backgroundColor: "{colors.surface-panel}"
    textColor: "{colors.primary}"
    rounded: "{rounded.md}"
    padding: 16px
  manifest-band:
    backgroundColor: "{colors.surface-coal}"
    textColor: "{colors.on-coal}"
    padding: 74px
---

# CortexFS DESIGN.md

Visual identity for CortexFS surfaces (docs-site, landing, demos). This file
follows the [DESIGN.md format](https://github.com/google-labs-code/design.md)
(`version: alpha`): YAML tokens are normative; prose explains how to apply them.

ABI and system architecture are **not** this document. See
[architecture.md](architecture.md) and [spec/](spec/).

## Overview

CortexFS should feel like a **precision instrument on warm paper** — closer to a
typeset systems manual and a dark terminal than to a SaaS marketing gradient.

Brand personality:

- **Engineered, not playful.** Kernel-adjacent seriousness; short words; no
  mascot fluff.
- **Warm substrate, cold tools.** Paper and limestone neutrals carry long-form
  reading; coal terminal surfaces carry commands and runtime proof.
- **One mint accent for life.** Interaction and status use mint; do not invent a
  rainbow of brand colors.
- **Type contrast.** Serif headlines for gravitas; Inter for UI and body;
  monospace for paths, commands, and instrumentation.

Audience: Linux and systems engineers, agent runtime authors, people who already
live in shells. UI density should favor scannable grids and readable code over
illustration-first layouts.

Emotional target: calm confidence — “this ABI is small enough to trust.”

## Colors

The palette is high-contrast ink on warm paper, with a single living accent.

- **Primary (#111312) — Ink:** Headlines, body emphasis, primary buttons, focus
  borders. Maximum permanence on paper.
- **Secondary (#66716C) — Moss slate:** Captions, secondary nav, muted lead
  copy. Never competes with ink.
- **Tertiary (#2A8F73) — Mint:** The sole interactive brand accent — eyebrows,
  links that mean “live,” brand italic accent, success-adjacent signals.
- **Neutral (#F7F5F1) — Paper:** Page foundation. Softer than pure white; pairs
  with limestone lines.
- **Surface (#FFFDFA) / panel (#FFFFFF):** Raised reading and card surfaces.
- **Coal (#181B19):** Terminal, demo frames, manifest band. On-coal text is
  warm off-white (#FFFDFA).
- **Line (#DED8CA):** Hairline structure; prefer borders over shadows.
- **Amber / Rose / Blue:** Semantic only (warning, danger, info) — not brand
  decoration.
- **Signal (#D8FF66):** Terminal highlight and “live rail” accents on coal only.

Dark theme inverts paper and ink while keeping mint, amber, rose, and blue as
semantic accents (see `docs-site/src/css/custom.css`). Agents implementing dark
mode should preserve token roles, not invent a second brand.

## Typography

Two narrative voices plus one instrument face:

- **Display / headlines — Georgia (serif):** Institutional, editorial, manual-
  like. Large sizes stay semi-bold (600–700), tight line-height, no tracking
  tricks.
- **Body / UI — Inter (sans):** 14–15px body, comfortable 1.65–1.68 line-height.
  Heavy weights (750–900) reserved for labels and nav, not body paragraphs.
- **Labels — Inter caps:** 12px, weight 900, `0.12em` letter-spacing, uppercase
  for section eyebrows only.
- **Mono — system monospace:** Commands, paths, object names (`model`, `agent`,
  `tool`, `session`), workbench chrome. Prefer weight 800–950 on coal so code
  reads as instrumentation, not decoration.

Do not introduce a third display family. Do not use monospace for marketing
headlines.

## Layout

Desktop is a **fluid grid capped near 1320px** content width with generous
horizontal rhythm. Hero and band sections use large vertical padding
(`band` ≈ 72px). Gutters between major columns are wide (`gutter` ≈ 54px).

Spacing scale is 4/8-based: `xs` 4 → `sm` 8 → `md` 16 → `lg` 24 → `xl` 32 →
`2xl` 48 → `3xl` 72. Prefer these steps over arbitrary pixel values.

Containment: related content lives in panel cards with `md` radius and internal
padding 18–20px. Feature rails and step grids share the same card language.
Avoid full-bleed colored hero washes; the page is paper first.

Mobile collapses multi-column hero and feature rails to a single column; primary
actions may shrink from the wide desktop CTA width.

## Elevation & Depth

Depth is **tonal and linear**, not skeuomorphic:

- Paper vs panel vs coal layers define hierarchy.
- Hairline borders (`line`) separate regions; shadows are rare and soft when
  present (e.g. demo stage `0 24px 70px` translucent black).
- Prefer border-color hover (ink) and 1–2px lift over heavy drop shadows.
- Terminal and workbench use inset light edges on coal, not outer glow.

Do not stack multiple competing elevations on one surface.

## Shapes

Shape language is **architectural soft-rect**:

- Default containers, code blocks, demo frames, feature cards: **8px** (`md`).
- Logo marks and tight chips: **6px** (`sm`).
- Nav pills and flow tags: **full** pill radius.
- No mixed “bubble UI” on the same screen as sharp engineering chrome.
- Brand logo tile sits on white with `sm` radius; keep mark legible on paper and
  coal.

## Components

### Buttons

- **Primary:** Ink fill, warm on-primary text, `md` radius, min-height 42px,
  heavy label weight. Optional diamond prefix (◆) on the home primary CTA only.
  Hover → pure black fill / white text.
- **Secondary / ghost panel:** Panel fill, ink text, line border. Hover
  strengthens border to ink and lifts 1px. Do not use mint fills for primary
  conversion actions.

### Nav

Navbar is frosted paper with a bottom line. Links are muted pills; active/hover
uses a faint ink wash. Brand title may collapse CorTeXfs → CTX → mark; keep
motion short and ease-out, respect `prefers-reduced-motion`.

### Cards and steps

Feature cards and developer steps share panel + line + `md` radius. Hover =
ink border + slight lift. Step code regions sit under a hairline with mono
type. Amber is allowed for step indices and feature codes, not for large fills.

### Terminal / workbench

Coal frames, mint/signal for prompts and live rails, mono instrumentation.
Traffic-light dots (rose / amber / mint) are chrome only. Transcript labels use
mint uppercase mono.

### Chips and proof row

Outline chips with soft mono text for object class names. Trust dots may use
semantic gradients; they are proof ornaments, not a second palette.

### Code blocks

Line border, `md` radius, no heavy shadow. Dark theme code sits on coal with
lighter border.

### Manifest band

Full coal band, large serif headline, muted on-coal supporting text, single
underlined text link — the “closing argument” surface.

## Do's and Don'ts

- **Do** treat YAML tokens as normative; change CSS only after tokens.
- **Do** keep mint as the only brand interaction accent on paper surfaces.
- **Do** put commands, paths, and ABI object names in monospace.
- **Do** prefer borders and tonal layers over decorative shadows.
- **Do** maintain WCAG AA contrast for body text on paper and on coal.
- **Don't** introduce purple SaaS gradients, glassmorphism stacks, or neon
  multi-accent dashboards.
- **Don't** use mint or signal as large background fills on paper.
- **Don't** set marketing headlines in monospace or body copy in Georgia at
  small sizes.
- **Don't** mix pill-everything with sharp-everything in one component group.
- **Don't** put provider logos or API-format branding into the root visual
  language — CortexFS is vendor-neutral.
- **Don't** confuse this file with the ABI: system design lives in
  [architecture.md](architecture.md) and [spec/](spec/).
