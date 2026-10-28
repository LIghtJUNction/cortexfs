# Public documentation site: extracted behavior

These requirements are static observations from the site source at the CortexFS
baseline commit. They were not browser-rendered during extraction. Apply visual
tokens from `../docs/DESIGN.md`; the external media registry in `../../ASSETS.md` identifies the original logo,
social card, demo poster, and video. No media files are included in this recipe.

## Routes and navigation

- `/` is the product landing page. `/reel` is a standalone product reel.
- The docs area contains the canonical ABI and user guides. Keep English and
  Simplified Chinese locale switching, with locale-appropriate document text.
- The navbar identifies `/ctx`, links to Docs, Install, Usage, `agent.sh`, the
  locale selector, and GitHub. External GitHub links open a new tab safely.
- Footer links cover install/use/develop/design/root ABI/object ABI and the
  ctx, agent.sh, chat UI, and session references.

## Landing page content and order

The page presents, in order: brand lockup and FUSE eyebrow; a serif headline
about agent runtimes not hiding in a database; supporting copy describing
models, agents, tools, and durable sessions mounted at `/ctx`; introduction,
install, and GitHub actions; a four-item proof ledger; a terminal-style line of
root names; interactive object curtain; introduction video; local product demo
and command/session evidence; four object cards; authority intersection; a
provider-neutrality section; three quick-start steps; and a closing link to the
specification.

The object cards explain these roles:

- `model`: reading returns metadata and execution performs one inference; API
  differences stay behind the neutral ABI.
- `agent`: executable object plus socket; Linux identity, mounts, cwd, model,
  and policy limit access.
- `tool`: executable capability invoked through `tsh`; resolution uses only
  `CTX_PATH`.
- `session`: append-only `messages.jsonl` and `events.jsonl`; prompt context is
  disposable and rebuildable.

The authority section expresses effective permission as the intersection of
mount visibility, UID/GID/mode, CortexFS policy, `CTX_PATH`, and noexec. The
provider-neutral section says provider/API detail does not expand root ABI.
Quick start steps show package installation, starting the systemd FUSE mount and
`ctx doctor`, then bootstrap/start/chat commands. Treat those commands as
snapshot copy; current repository no longer contains their implementation.

The hero embeds an introduction video using the privacy-enhanced YouTube
endpoint for video `3BhCiHWbrUQ`, with a caption. When the separately supplied media is available, the product demo uses
WebM first and MP4 second, has native controls, starts muted/inline,
preloads metadata, displays the poster, and includes an MP4 fallback link.
Captions identify it as a real CLI/agent demo and expose the command
`ctx agent start coder` followed by `ctx agent chat coder`, plus the durable
session path `/ctx/home/<uid>/agent/coder/session/default/messages.jsonl`.

## Interactive object curtain

The curtain has four keyboard-operable buttons: `model`, `agent`, `tool`, and
`session`. Selecting one updates its pressed state, visible object specimen,
and polite live caption. The drawing shows the `/ctx` FUSE mount plate and a
root-name line. Pointer movement can peel/move specimen columns. Disable this
pointer animation for reduced-motion preference and coarse-pointer devices;
pointer leave resets the scene. The decorative visual is hidden from assistive
technology, while the scene and button group have accessible labels.

Snapshot captions:

- model: “A model is a pure inference file: read metadata, execute one
  inference.”
- agent: “An agent is an executable object and socket: a policy-bound
  orchestrator.”
- tool: “A tool is a capability endpoint resolved and executed by tsh through
  CTX_PATH.”
- session: “A session is ordinary files: raw JSONL stays durable; prompt
  context rebuilds.”

## Reel route

The reel fills the viewport, locks page scrolling, and runs a 20-second loop.
`?autoplay=0` leaves the animation paused; the default enables it. Preserve the
five content phases: stable root names; three executable objects; a live chat
command with session path and `tsh → fs.read`; the authority intersection; and
four specimen roles with the closing statement. A mint progress rail tracks the
loop. Respect reduced motion. At aspect ratios narrower than 4:3, move the
assembly and phase copy inward, hide the center header label, and use a
two-column header. All visible phase text is localized.

The original page and reel enumerate seven roots and omit `channel`, while the
baseline implementation and detailed ABI specify eight roots. This is a known
content conflict: the product surface must implement eight (see `REQ-001`); do
not silently use the old marketing list as its filesystem contract. For an
exact site reproduction, preserve or explicitly correct the page copy and state
which choice was made.
