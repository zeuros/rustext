# rustext

A Monaco-class code editor and diff editor in Rust, meant to replace Monaco in
[gitgud](../gitgud) when gitgud moves to full Rust.

Start here, in this order:
1. `docs/PLAN.md`: milestones, ticked boxes, and **"Where to resume"** at the bottom.
2. `docs/DECISIONS.md`: why things are the way they are (D1…). Change a decision there before
   changing the code it governs.
3. `docs/MONACO_FEATURES.md`: every Monaco feature with its API, priority and status.

## Commands

- `scripts/check.sh`: what CI runs (fmt check, clippy `-D warnings`, tests), stopping at the first failure. Run it before every commit; `cargo fmt --all` fixes formatting.
  Commit in a separate command after it printed success: chaining `check.sh; …; git commit` in one
  shell line commits broken code when the check fails.
- Scripted edits (python `str.replace`) must assert the old text is present: rustfmt reflows long
  lines, and a silent no-op replacement has already lost two edits in this repository.
- `cargo run -p rustext-demo --release`: the playground app. `-- <file>` opens a file; `-- --diff <old> <new>` a diff.
- Screenshots without touching the desktop: run the demo on a private Xvfb
  (`scripts/screenshot.sh <out.png> [demo args…]`), then read the PNG.
- Widget tests use `egui_kittest` (headless, no GPU).

## Layout

| Crate | Holds |
|---|---|
| `crates/rustext-core` | `Text` (ropey), `Position`/`Range`, `Selection`/`Selections`, `ChangeSet`, `Transaction`, `History`, `Command`, `EditorState`, motions, edits, line ops, comments, multi-cursor |
| `crates/rustext-syntax` | languages and tokenizing (syntect + two-face grammars), themes |
| `crates/rustext-diff` | line diff, inner changes, aligned rows, collapsed regions, patches for staging |
| `crates/rustext-egui` | `CodeEditor`, `DiffEditor` and `MergeEditor` widgets |
| `crates/rustext-lsp` | a Language Server Protocol client implementing `LanguageServices` |
| `crates/rustext-demo` | the playground |

Only `rustext-egui` and `rustext-demo` know about egui. Anything decidable without pixels goes
in the other crates, with unit tests.

## Conventions

From gitgud's, adapted to Rust:

- **KISS, DRY, functional.** Commands are pure functions returning a `Transaction`;
  values (`Text`, `Selections`, `ChangeSet`) are immutable; `EditorState::execute`/`apply` is
  the one place that mutates editor state. No global state (D6).
- **Avoid Monaco's design mistakes** (DECISIONS D5, D6, D8, D10, D11, D12): no 1-based or
  UTF-16 coordinates, no global registries or themes, no string command ids, no listener
  soup, no CSS-class decorations, no 200-key option bag.
- **Names say what the value is**, with its kind or unit: `line_start`, `char_offset`,
  `visual_column`, `scroll_offset_px`. No abbreviations or one-letter names, except loop
  indexes and short closure parameters. If a name needs a comment, rename it.
- One term per concept across code, tests and docs: *anchor/head* (not start/end) for
  selections, *line ending* (not EOL), *char offset* vs *byte offset*, *column* (chars) vs
  *visual column* (tabs expanded).
- Comments say what the code does or why, never its history.
- Inside a function, a blank line separates its steps.
- Tests: one behaviour per test, named after it (`backspace_in_indentation_goes_back_a_tab_stop`).
- Commits: conventional (`feat(core): …`, `fix(diff): …`, `docs: …`), small and focused.
  No AI attribution lines.

## Keeping the docs current

At the end of a session (or when a milestone lands): tick boxes in `docs/PLAN.md`, update
statuses in `docs/MONACO_FEATURES.md`, rewrite "Where to resume". A new design choice
gets a new `D<n>` in `docs/DECISIONS.md`.
