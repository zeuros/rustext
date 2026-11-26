# Plan and milestones

Read [DECISIONS.md](DECISIONS.md) for the why, [MONACO_FEATURES.md](MONACO_FEATURES.md) for
the full feature list. This file is the order of work. Tick boxes as things land, and keep
"Where to resume" at the bottom current at the end of every session.

A milestone is done when: its boxes are ticked, `cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings` pass, and the demo shows the feature.

## M0. Project setup
- [x] Cargo workspace with `rustext-core`, `rustext-syntax`, `rustext-diff`, `rustext-egui`, `rustext-demo`
- [x] CI: fmt, clippy `-D warnings`, tests
- [x] CLAUDE.md, README, docs

## M1. Text model (`rustext-core`)
- [x] `Text` over ropey: lines split on `\n` only, `line_content` without `\r\n`, `LineEnding::detect`
- [x] `Position`, `Range`, `Selection` (anchor/head), `Selections` (normalized: sorted, merged, one primary)
- [x] `Change` / `ChangeSet`: apply, invert, compose, `map_position(Bias)`
- [x] `Transaction`: change set + selections after it
- [x] Graphemes: next/previous boundary on a line; word classes and word boundaries (`word_separators`)
- [x] Indentation: detect (tabs/spaces, size), indent unit, visual columns with tabs

## M2. Editor state and commands (`rustext-core`)
- [x] `EditorState { document, selections, history, config }` with pure `apply(Command) -> EditorState`
- [x] Cursor motions: char, grapheme, word (start/end/part), line start (smart home)/end, doc start/end, page; with `select` variants; sticky visual column
- [x] Typing, backspace, delete (char, word), enter, tab; over every cursor
- [x] History: undo / redo, typing coalesced in groups, selections restored, `is_dirty` against a saved version
- [x] Clipboard commands: copy / cut whole line when the selection is empty; paste spread over cursors when the counts match
- [x] Multi-cursor: add above/below, add next occurrence (Ctrl+D), select all occurrences, cursors at line ends, column selection
- [x] Read-only mode: editing commands rejected, `EditorResponse` says so

## M3. `CodeEditor` widget v1 (`rustext-egui`) + demo
- [x] Paint visible lines, gutter with line numbers (absolute/relative/off), current line highlight
- [x] Cursors (line, block, underline; blinking), selections
- [x] Scrolling (wheel, Alt fast scroll, scrollbars, scroll beyond last line), reveal cursor with margin
- [x] Mouse: click, drag-select, double-click word, triple-click line, Alt+click adds a cursor, Shift+click extends, Alt+Shift drag column select, gutter click selects line
- [x] Keyboard via a `Keymap` value (VS Code defaults), clipboard, IME composition
- [x] `EditorResponse` (D10): changed, selection changed, clicked target
- [x] Demo app with a sample file and option toggles
- [x] Font zoom (Ctrl+wheel, Ctrl +/-)

## M4. Syntax highlighting (`rustext-syntax`)
- [x] Language detection by file name / extension / first line, via two-face grammars
- [x] Incremental line tokenizer with cached states, re-tokenize from the edited line — lazily up to the painted lines; stopping early when states match is not done
- [x] Themes as values (D6): token rules + typed UI colors; built-in light, dark, high-contrast; gitgud's light and dark
- [x] Load `.tmTheme` / VS Code JSON themes — `.tmTheme` and two-face's ~30 themes; VS Code JSON not yet
- [x] Widget paints tokens; huge files: highlight visible part first, plain text past a limit

## M5. Diff core (`rustext-diff`)
- [x] Line diff (histogram), `ignore_trim_whitespace`, size/time limits
- [x] Inner changes per hunk (words then chars), like Monaco's `charChanges`
- [x] Side-by-side alignment rows (padding where a side has no line)
- [x] Inline (unified) rows
- [x] Collapsed unchanged regions with context lines, reveal more
- [x] Next / previous change navigation
- [x] Patch builder: unified diff of a hunk, of selected lines (stage, unstage, discard), port of gitgud's `monaco-patch-builder.ts` and its tests

## M6. `DiffEditor` widget
- [x] Side-by-side with synchronized scroll and padding rows
- [x] Inline view
- [x] Hunk view (collapsed unchanged regions, click to expand)
- [x] Line / inner change backgrounds from theme, +/- indicators, overview ruler of changes
- [x] Resizable split, inline when narrow — inline below a width; the split ratio is a config value, not draggable yet
- [x] Line selection on either side → `DiffResponse` with the selected lines (for staging)
- [x] Context menu hooks (application-provided items) — each side's `EditorResponse.response` takes egui's `context_menu`

## M7. Editing power
- [x] Line ops: move, copy, delete, duplicate, join, sort, reverse, unique, trim trailing whitespace
- [x] Comments: toggle line comment, block comment (language configuration)
- [x] Indent / outdent, auto-indent on Enter (indent rules, brackets), Tab inserts indent unit
- [x] Auto-closing pairs, overtype closing char, delete pair, surround selection
- [x] Case transforms (upper, lower, title, snake, camel, kebab, pascal)
- [x] Find / replace: model search (case, whole word, regex, in selection), widget, replace all with groups, select all matches
- [x] Go to line

## M8. Decorations, folding, markers
- [x] `DecorationSet` mapped through edits; typed styles (D11); whole-line, inline, gutter, overview ruler
- [x] Hidden areas in a display map (buffer line ↔ display row)
- [x] Indentation folding, fold markers in the gutter, fold/unfold/all/level N, `…` placeholder
- [x] Markers (diagnostics) with squiggles, hover text, next/previous problem
- [x] Blame demo (shaded line groups, click on a line number)

## M9. Rendering extras
- [x] Whitespace rendering modes, control characters, visible `␍`
- [x] Rulers, indent guides with active guide
- [x] Matching bracket highlight, jump to bracket; bracket pair colorization — brackets inside strings and comments are colored too (needs token scopes from the styler)
- [x] Selection and word occurrence highlights
- [x] Word wrap (off, viewport, column, bounded) with wrapping indent, in the display map — continuation rows indented like their line (`wrappingIndent: same`); Up/Down move by line, not by visual row (D20)
- [ ] View zones (blocks between lines) in the display map

## M10. Merge editor
- [x] Three-way model: base, ours, theirs → conflicts and non-conflicting changes
- [x] Widget: two inputs + result, accept ours / theirs / both per conflict, accept all of a side

## M11. Language services (D9)
- [x] `LanguageServices` trait, versioned async results — `Reply::Pending` makes the editor ask again on the next frames
- [x] Completion widget (fuzzy filter, details, snippets with tab stops) — no snippets / tab stops yet; word-based completions built in
- [x] Hover, signature help, go to definition (Ctrl+click), document highlights, rename — hover and definition (F12, Ctrl+click, same text) done; signature help, rename not yet
- [ ] LSP client implementation (stdio)

## M12. Extras
- [ ] Minimap
- [x] Sticky scroll (indentation headers, up to 5 pinned; click to go back)
- [ ] Command palette
- [ ] Inlay hints, code lens, inline completions (ghost text)

## M13. Gitgud integration
- [ ] An adapter matching gitgud's uses (table in MONACO_FEATURES.md)
- [x] Performance: 100k-line file opens and scrolls at 60 fps (see D19 for the numbers); 10k-line diff in 4 ms, 100k in 45 ms (D14)

## Where to resume

Last updated: 2026-11-26, end of the first session.

**State**: M0–M7 done; M8/M9 mostly done (see unticked boxes). 177 tests, `scripts/check.sh`
green. Every milestone is a commit on `main` (`git log --oneline`).

**Next, in order** (each a commit):
1. (done since: diagnostics with hover and F8, the blame demo, the merge editor checked
   against `git merge-file`.)
2. (done since: bracket pair colorization, word wrap — D20, performance work — D19.)
5. M11: an LSP client implementing `LanguageServices` (stdio JSON-RPC, positions converted to UTF-16 at that boundary — D5); snippets. M12: minimap, sticky scroll, command palette.

**Known gaps / debts**:
- Tokenizer re-tokenizes from the edited line to the painted lines; Monaco stops when the
  end state matches the cached one (worth it for huge files).
- Folding is indentation-only; `foldingStrategy: 'auto'` needs language structure.
- The split ratio of the diff editor is not draggable.
- `CodeEditor` folding is off when the application passes its own `DisplayLayout` (diffs).
- Find widget: find in selection (`FindState::scope`) has no toggle button yet.

**How to verify visually**: `scripts/screenshot.sh <png> [--showcase | --find x | --page diff |
--inline | --diff old new]`, then look at the PNG.
