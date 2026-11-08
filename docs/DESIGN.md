---
version: alpha
name: CortexFS documentation
description: Quiet technical documentation with clear navigation, generous space, and one blue link accent.
colors:
  dark:
    background: "#0B0B0B"
    surface: "#171717"
    text: "#E8EAED"
    muted: "#A1A1AA"
    link: "#8AB4F8"
    line: "#303030"
  light:
    background: "#FFFFFF"
    surface: "#F6F8FA"
    text: "#202124"
    muted: "#5F6368"
    link: "#1967D2"
    line: "#DADCE0"
typography:
  body:
    fontFamily: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 16px
    fontWeight: 400
    lineHeight: 1.75
  heading:
    fontFamily: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 32px
    fontWeight: 600
    lineHeight: 1.25
  code:
    fontFamily: ui-monospace, SFMono-Regular, Consolas, monospace
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.6
spacing:
  unit: 8px
  mobile-gutter: 20px
  desktop-gutter: 40px
  section-gap: 48px
  reading-width: 76ch
  page-max: 1120px
rounded:
  control: 6px
---

# Documentation design

[Documentation](README.md) · [Architecture](architecture.md)

This guide defines the presentation of the maintained documentation. The
website implementation is absent; these tokens describe a future renderer.
GitHub controls its own Markdown appearance. The recipe's
[earlier visual contract](../recipes/cortexfs/contracts/docs/DESIGN.md) remains
part of its fixed reconstruction snapshot.

## Reading comes first

Use one content column, a system sans-serif face, and comfortable line spacing.
Let headings and whitespace group information. Reserve background panels for
code or a necessary notice; ordinary paragraphs and navigation need no cards.

Keep the opening short: title, one-sentence purpose, useful links, and any
version boundary that changes how the page should be used. Omit cover art,
badge walls, promotional slogans, and repeated feature lists from entry pages.

## Navigation and content

The [documentation index](README.md) groups pages by task: reconstruct,
understand the design, inspect contracts, and consult historical operations.
A page should link back to its index and to the next useful reference.

Give each subject one authoritative home. README introduces the project;
architecture explains responsibilities; internal architecture records process
boundaries; the specification owns paths and wire formats. Link to a detailed
contract instead of copying it into every guide.

Write one point per paragraph and state it in the first sentence. Use precise
verbs and define terms before relying on them. Use tables for comparisons and
lookups, numbered steps for ordered work, and code blocks for copyable commands
or formats. Keep prose and navigation outside code blocks.

English and Chinese entry pages should offer the same tasks. Translate content
when a real translation is available; otherwise link to the English original
and label its language. Preserve page names and heading anchors used by other
pages, or update their incoming links in the same change.

## Theme and layout

Use the dark tokens for a near-black reading surface and the light tokens for
white. Respect the reader's chosen theme. Blue identifies links and focus;
status also needs a text label. Keep borders subtle and limited to places that
need a separator. Avoid shadows, gradients, floating ornaments, and animated
backgrounds.

Keep prose near 76 characters per line and the page within 1120px. Use 20px
mobile gutters and 40px desktop gutters. Headings may scale from 28px on small
screens to 32px on larger screens. Use the 8px spacing unit and 48px between
major sections. Code stays regular weight.

On narrow screens, stack navigation and content. Let wide code and tables
scroll within their own region while the document stays inside the viewport.
Use short link labels for commit hashes and paths; the link retains the exact
reference.

## Commands and evidence

State whether an example applies to this checkout, a historical source
snapshot, or a reconstruction target. Pin historical download URLs and any
source revision selected by the downloaded script. Place prerequisites before
a command and describe its result immediately after it.

Tie product claims to a source contract or an executed result with its version
and environment. A target, a historical result, and a result from this checkout
each need their own label. Put that information near the relevant claim.

## Accessibility and review

Use semantic headings without skipped levels, descriptive link text, and
visible keyboard focus. Aim for at least 4.5:1 contrast for ordinary text and
44px interactive targets in a website renderer. Underline links in prose;
communicate status with words as well as color. Honor reduced-motion settings
where interaction requires motion.

Before delivery, check local links and anchors, bilingual scope, readable code
and tables, and the provenance of changed claims. For a website implementation,
also inspect narrow and wide viewports in both themes and test keyboard
navigation. Report only the surfaces that were actually rendered and checked.
