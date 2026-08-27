# scratch_design — TEMPORARY

Imported from the Claude Design project **"TUI design system"**
(`claude.ai/design/p/5dd6ae65-ce65-434b-9898-6d5fb637a140`) as the reference for
the M11 ratatui TUI.

**This directory is scratch. Delete it once M11 is done.** Nothing in the crate
reads from it; it is here so the design and the implementation land in the same
history while the port is in progress.

## What is authoritative

`Mooshik TUI.dc.html` — nine artboards (`1a`–`1i`). Each is a true character
grid: every panel carries `col`/`row`/`w`/`h` in cells, and every text block is
placed at `left:calc(var(--cw) * N); top:calc(var(--ch) * M)` — i.e. column `N`,
row `M`. That makes the file a precise spec, not a mockup, and it is what
`src/tui` is built from.

- `1a` Today — the default screen (120x40)
- `1b` The week — seven days plus the threads running across them
- `1c` Something came back — recall provenance inline in the conversation
- `1d` A quiet caution — an inline yellow panel, not a modal
- `1e` First run — five fields, validated in place
- `1f` Changing the database — the one double-ruled box, the only red
- `1g` The same field, a cosmetic edit — the no-warning counterpart to `1f`
- `1h` Narrow — 80x24
- `1i` Colour legend & strength notation — **the palette authority**

`Panel.dc.html` — the frame component every artboard imports: box-drawing
border with the title inset at column 2 over the panel's own background.
Ratatui's `Block` with a title does exactly this.

## What is not

`_ds/nocturne-*/` and `support.js` are the *web* presentation layer — the
Nocturne design system that styles the artboard page chrome (headings, captions,
cards) and the `dc-runtime` that renders `.dc.html` in a browser. Neither
describes the terminal. The terminal palette is 16-colour ANSI plus the dim
attribute, defined inline in each artboard and documented in `1i`.

`_ds_bundle.js` is an empty stub (the project defines no DS components).

Not imported: `screenshots/check-1a.png` (a design-tool self-check render) and
`.thumbnail`.
