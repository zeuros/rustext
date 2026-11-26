# Plan and milestones

Read [DECISIONS.md](DECISIONS.md) for the why, [MONACO_FEATURES.md](MONACO_FEATURES.md) for
the full feature list. This file is the order of work. Tick boxes as things land, and keep
"Where to resume" at the bottom current at the end of every session.

A milestone is done when: its boxes are ticked, `cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings` pass, and the demo shows the feature.

## M0. Project setup
- [ ] Cargo workspace with `rustext-core`, `rustext-syntax`, `rustext-diff`, `rustext-egui`, `rustext-demo`
- [ ] CI: fmt, clippy `-D warnings`, tests
- [ ] CLAUDE.md, README, docs

## M1. Text model (`rustext-core`)
- [ ] `Text` over ropey: lines split on `\n` only, `line_content` without `\r\n`, `LineEnding::detect`
- [ ] `Position`, `Range`, `Selection` (anchor/head), `Selections` (normalized: sorted, merged, one primary)
- [ ] `Change` / `ChangeSet`: apply, invert, compose, `map_position(Bias)`
- [ ] `Transaction`: change set + selections after it
- [ ] Graphemes: next/previous boundary on a line; word classes and word boundaries (`word_separators`)
- [ ] Indentation: detect (tabs/spaces, size), indent unit, visual columns with tabs

## M2. Editor state and commands (`rustext-core`)
- [ ] `EditorState { document, selections, history, config }` with pure `apply(Command) -> EditorState`
- [ ] Cursor motions: char, grapheme, word (start/end/part), line start (smart home)/end, doc start/end, page; with `select` variants; sticky visual column
- [ ] Typing, backspace, delete (char, word), enter, tab; over every cursor
- [ ] History: undo / redo, typing coalesced in groups, selections restored, `is_dirty` against a saved version
- [ ] Clipboard commands: copy / cut whole line when the selection is empty; paste spread over cursors when the counts match
- [ ] Multi-cursor: add above/below, add next occurrence (Ctrl+D), select all occurrences, cursors at line ends, column selection
- [ ] Read-only mode: editing commands rejected, `EditorResponse` says so

## M3. `CodeEditor` widget v1 (`rustext-egui`) + demo
- [ ] Paint visible lines, gutter with line numbers (absolute/relative/off), current line highlight
- [ ] Cursors (line, block, underline; blinking), selections
- [ ] Scrolling (wheel, Alt fast scroll, scrollbars, scroll beyond last line), reveal cursor with margin
- [ ] Mouse: click, drag-select, double-click word, triple-click line, Alt+click adds a cursor, Shift+click extends, Alt+Shift drag column select, gutter click selects line
- [ ] Keyboard via a `Keymap` value (VS Code defaults), clipboard, IME composition
- [ ] `EditorResponse` (D10): changed, selection changed, clicked target
- [ ] Demo app with a sample file and option toggles
- [ ] Font zoom (Ctrl+wheel, Ctrl +/-)

## M4. Syntax highlighting (`rustext-syntax`)
- [ ] Language detection by file name / extension / first line, via two-face grammars
- [ ] Incremental line tokenizer with cached states, re-tokenize from the edited line until states match
- [ ] Themes as values (D6): token rules + typed UI colors; built-in light, dark, high-contrast; gitgud's light and dark
- [ ] Load `.tmTheme` / VS Code JSON themes
- [ ] Widget paints tokens; huge files: highlight visible part first, plain text past a limit

## M5. Diff core (`rustext-diff`)
- [ ] Line diff (histogram), `ignore_trim_whitespace`, size/time limits
- [ ] Inner changes per hunk (words then chars), like Monaco's `charChanges`
- [ ] Side-by-side alignment rows (padding where a side has no line)
- [ ] Inline (unified) rows
- [ ] Collapsed unchanged regions with context lines, reveal more
- [ ] Next / previous change navigation
- [ ] Patch builder: unified diff of a hunk, of selected lines (stage, unstage, discard), port of gitgud's `monaco-patch-builder.ts` and its tests

## M6. `DiffEditor` widget
- [ ] Side-by-side with synchronized scroll and padding rows
- [ ] Inline view
- [ ] Hunk view (collapsed unchanged regions, click to expand)
- [ ] Line / inner change backgrounds from theme, +/- indicators, overview ruler of changes
- [ ] Resizable split, inline when narrow
- [ ] Line selection on either side → `DiffResponse` with the selected lines (for staging)
- [ ] Context menu hooks (application-provided items)

## M7. Editing power
- [ ] Line ops: move, copy, delete, duplicate, join, sort, reverse, unique, trim trailing whitespace
- [ ] Comments: toggle line comment, block comment (language configuration)
- [ ] Indent / outdent, auto-indent on Enter (indent rules, brackets), Tab inserts indent unit
- [ ] Auto-closing pairs, overtype closing char, delete pair, surround selection
- [ ] Case transforms (upper, lower, title, snake, camel, kebab, pascal)
- [ ] Find / replace: model search (case, whole word, regex, in selection), widget, replace all with groups, select all matches
- [ ] Go to line

## M8. Decorations, folding, markers
- [ ] `DecorationSet` mapped through edits; typed styles (D11); whole-line, inline, gutter, overview ruler
- [ ] Hidden areas in a display map (buffer line ↔ display row)
- [ ] Indentation folding, fold markers in the gutter, fold/unfold/all/level N, `…` placeholder
- [ ] Markers (diagnostics) with squiggles, hover text, next/previous problem
- [ ] Blame demo (shaded line groups, click on a line number)

## M9. Rendering extras
- [ ] Whitespace rendering modes, control characters, visible `␍`
- [ ] Rulers, indent guides with active guide
- [ ] Matching bracket highlight, jump to bracket; bracket pair colorization
- [ ] Selection and word occurrence highlights
- [ ] Word wrap (off, viewport, column, bounded) with wrapping indent, in the display map
- [ ] View zones (blocks between lines) in the display map

## M10. Merge editor
- [ ] Three-way model: base, ours, theirs → conflicts and non-conflicting changes
- [ ] Widget: two inputs + result, accept ours / theirs / both per conflict, accept all of a side

## M11. Language services (D9)
- [ ] `LanguageServices` trait, versioned async results
- [ ] Completion widget (fuzzy filter, details, snippets with tab stops)
- [ ] Hover, signature help, go to definition (Ctrl+click), document highlights, rename
- [ ] LSP client implementation (stdio)

## M12. Extras
- [ ] Minimap
- [ ] Sticky scroll
- [ ] Command palette
- [ ] Inlay hints, code lens, inline completions (ghost text)

## M13. Gitgud integration
- [ ] An adapter matching gitgud's uses (table in MONACO_FEATURES.md)
- [ ] Performance: 100k-line file opens and scrolls at 60 fps; 10k-line diff under 50 ms

## Where to resume

(Updated at the end of each session: what was done last, what is half done, what's next.)
