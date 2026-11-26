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
- [x] Incremental line tokenizer with cached states, re-tokenize from the edited line — the cache follows `EditedLines` and re-tokenizing stops where the end state matches the cached one
- [x] Themes as values (D6): token rules + typed UI colors; built-in light, dark, high-contrast; gitgud's light and dark
- [x] Load `.tmTheme` / VS Code JSON themes — `.tmTheme` and two-face's ~30 themes; VS Code JSON not yet
- [x] Widget paints tokens; huge files: highlight visible part first, plain text past a limit
- [x] Tree-sitter for the languages with a bundled grammar (D22): incremental, parsed on a thread, highlight queries mapped to TextMate scopes; smart select from its nodes

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
- [x] Resizable split, inline when narrow — inline below a width; the separator drags
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
- [x] Word wrap (off, viewport, column, bounded) with wrapping indent, in the display map — continuation rows indented like their line (`wrappingIndent: same`); Up/Down move by visual row (D20)
- [ ] View zones (blocks between lines) in the display map

## M10. Merge editor
- [x] Three-way model: base, ours, theirs → conflicts and non-conflicting changes
- [x] Widget: two inputs + result, accept ours / theirs / both per conflict, accept all of a side

## M11. Language services (D9)
- [x] `LanguageServices` trait, versioned async results — `Reply::Pending` makes the editor ask again on the next frames
- [x] Completion widget (fuzzy filter, details, snippets with tab stops) — snippets with tab stops, mirrors and `$0`; word-based completions built in
- [x] Hover, signature help, go to definition (Ctrl+click), document highlights, rename — hover, definition (F12, Ctrl+click, same text), rename (F2), signature help, document highlights done
- [x] LSP client implementation (stdio) — `rustext-lsp`: completion, hover, definition, full document sync; tested against a fake server binary; playground `--lsp "<server>"`

## M12. Extras
- [x] Minimap (token-colored runs, viewport slider, click or drag to scroll; off by default)
- [x] Sticky scroll (indentation headers, up to 5 pinned; click to go back)
- [x] Command palette (F1, Ctrl+Shift+P; every keymap binding by name with its keys)
- [x] Inlay hints, code lens, inline completions (ghost text) — injected text decorations, inlay hints from `LanguageServices::inlay_hints` (LSP too) and ghost text from `LanguageServices::inline_completion`; code lens not yet

## M13. Gitgud integration
- [x] An adapter matching gitgud's uses (table in MONACO_FEATURES.md) — `docs/GITGUD_INTEGRATION.md` maps each Monaco use to the API; `tests/gitgud_snippets.rs` keeps it compiling
- [x] Performance: 100k-line file opens and scrolls at 60 fps (see D19 for the numbers); 10k-line diff in 4 ms, 100k in 45 ms (D14)

## Where to resume

Last updated: 2026-11-26, after moving to Helix's crates (D22–D24).

**State**: M0–M10 done, M11 and M12 mostly done, M13 done (integration guide, performance).
`scripts/check.sh` green; ~280 tests, including seeded random tests (`random_commands.rs`
in core, `random_diffs.rs` in diff, `random_events.rs` in egui: raise their case counts or
change their seeds locally to hunt for rarer bugs). Every feature is a commit on `main`.

**Next, in order** (each a commit):
1. Tree-sitter for bracket matching and colorization, replacing the lexer (D21) where a
   grammar is bundled (folding by syntax is done).
2. An accessible diff viewer (`accessibleDiffViewerNext`): the changes read one by one.

Done since: format document (Shift+Alt+F); Go to Symbol (Ctrl+Shift+O); word wrap in diffs (`--page diff --wrap N`); links (underlined URLs, Ctrl+click opens); the visible lines and the selection exposed to screen readers (AccessKit text
runs); definitions and references in other documents (`open_request`, D26), F12 answered on a later frame; bracket pair guide of the cursor's pair (`--bracket-guides`); snippet placeholders highlighted; folding by syntax tree (`FoldingStrategy`), Fold from inside a block moves the
cursor to its header; code lenses (D25, `--code-lens` in the demo); Helix's crates (D22–D24): tree-sitter highlighting for 18 languages, parsed on a
thread; smart select (Shift+Alt+→/←); `lsp-types` messages, checked against a real
rust-analyzer (`crates/rustext-lsp/tests/rust_analyzer.rs`, ignored); `nucleo` fuzzy matching;
`regex-cursor` find over the rope.

Done before that: same-text services (diagnostics, rename, signature
help, document highlights, inlay hints, references on Shift+F12) with incremental LSP sync;
brackets in strings and comments (D21); Up/Down by visual row when wrapping; snippets; ghost
text; random tests.

**Known gaps / debts**:
- Folding is indentation-only; `foldingStrategy: 'auto'` needs language structure.
- Folding is off when the application passes its own `DisplayLayout` (diffs).
- Text from egui's TextEdit (find input) can't use rustext's keymap (Ctrl+D etc. there are
  egui's).
- The bracket lexer (D21) doesn't know raw strings, nested comments or heredocs.
- Find References isn't visible in the demo without `--lsp` (word completions answer none).
- The references list shows `file:line` for other documents, not their line's text.
- Tree-sitter: bracket matching and indentation could use the tree (D21's lexer is still
  used); injections aren't run; highlight queries are the grammar crates' own.

**How to verify visually**: `scripts/screenshot.sh <png> [file] [--showcase | --find x |
--page diff | --page merge | --inline | --diff old new | --blame | --wrap N | --complete |
--minimap | --scroll N | --code-lens | --bracket-guides]`, then look at the PNG. Performance: `cargo test --release -p
rustext-egui --test performance -- --ignored --nocapture` (numbers in D19).
