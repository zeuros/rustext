# Design decisions

Each decision: what was chosen, why, and what was rejected. Number them; never renumber.
When a decision is changed, edit it in place and say why the new choice is better (no history log).

## D1. Goal and scope

A code editor and diff editor component in Rust, as good as Monaco for what gitgud needs
(diff view, staging hunks and lines, blame, merge editor), then for general editing.
Gitgud is meant to move from Tauri + Angular + Monaco to full Rust, so the editor is a
native widget, not a web view.

## D2. Workspace layout: a UI-agnostic core, thin UI crates

| Crate | Holds | Depends on UI? |
|---|---|---|
| `rustext-core` | text, positions, selections, edits, history, commands, find, folding, decorations, language configuration | no |
| `rustext-syntax` | tokenizing (syntect grammars), themes' token colors | no |
| `rustext-diff` | line + inner diff, hunks, aligned views, unchanged-region collapsing, patch building | no |
| `rustext-egui` | the `CodeEditor` and `DiffEditor` widgets | egui |
| `rustext-demo` | a playground app, like Monaco's playground | eframe |

Everything that can be decided without pixels lives in the core crates and is unit tested.
The UI crate turns state into shapes and input events into commands. A second UI
(gpui, iced, a TUI) would only reimplement `rustext-egui`.

## D3. UI toolkit: egui (for now)

- Immediate mode fits an editor: we paint every line ourselves anyway (Monaco does too),
  and "the widget returns what happened this frame" replaces Monaco's event listeners (D10).
- Mature, portable (Linux, Windows, macOS, web via wasm), small dependency, good IME support,
  AccessKit integration, and `egui_kittest` for headless widget tests.
- Rejected for now: **gpui** (Zed's, best text quality, but its API follows Zed's needs and
  changes often), **iced** (Elm architecture is nice but custom text rendering is harder),
  **floem** (Lapce's, smaller community), **Tauri + web**: that is what we leave.
- This is the decision most likely to be revisited when gitgud's own toolkit is chosen;
  D2 keeps it cheap.

## D4. Text storage: `ropey` 1.x behind our own `Text` type

- O(log n) edits and line lookups, cheap clones by structural sharing: a `Text` is a
  value. Every edit returns a new `Text` and old versions stay valid, which makes undo,
  diffing against a previous version, and functional state updates free.
- Wrapped in `rustext_core::Text` so the rest of the code never calls ropey directly and the
  storage can change (ropey 2 is in beta; Zed's sum-tree rope is the other candidate).
- Lines are split on `\n` only (`\r\n` is a `\n` with a `\r` before it; see D5). Ropey's
  default would also break on `\r`, U+2028 and others, which makes columns lie.

## D5. Coordinates: 0-based lines, columns in chars, line endings kept as they are

Monaco: 1-based lines and columns, columns counted in UTF-16 code units, `\r\n` normalized
away on load (gitgud had to replace `\r` with `␍` to see them).

Here:
- `Position { line, column }`, both **0-based**; `column` counts Unicode scalar values (`char`).
  0-based removes the `+ 1` / `- 1` everywhere; chars match ropey and Rust strings
  (convert to UTF-16 only at an LSP boundary).
- Cursor motions step over grapheme clusters (an emoji with modifiers is one step),
  using `unicode-segmentation`.
- The text keeps the exact bytes it was given: a `\r` before `\n` stays in the text,
  `line_content` returns the line without its `\n` nor its `\r`, and
  `LineEnding::detect` tells which one the file uses. A diff therefore sees CRLF ↔ LF
  changes, and the renderer can show `␍` without changing the text.
- Inside the core, selections and changes use **char offsets** into the text (like Helix):
  mapping through edits is then one subtraction, with no line bookkeeping. `Position` is for
  the API surface, display and tests; `Text::offset` / `Text::position` convert.
- `Range { start, end }` is always ordered (`start <= end`). A `Selection` is
  `{ anchor, head }`: where the selection started and where the cursor is. Monaco's
  `Selection` has four numbers and a `getDirection()`; Monaco's `Range` can be built
  reversed and is then silently swapped.

## D6. No global state

Monaco keeps global registries: models by URI (`createModel`, `getModel`), one theme for
every editor on the page (`setTheme`), language providers keyed by language id, markers by
owner and URI. Two editors can't have different themes; tests leak state into each other.

Here every editor owns or borrows what it uses: its `Document`, its `Theme`, its
`Language`. Sharing is explicit: two editors showing the same document hold the same
`Document` value (or the application syncs them). A registry of languages exists
(`LanguageRegistry`), but it is a value passed in, not a static.

## D7. Functional core: values and pure transitions

- `Text`, `Selection`, `Selections`, `ChangeSet`, `Decoration` are immutable values.
- Every command is a pure function `fn(&EditContext) -> Option<Transaction>` (a `ChangeSet`
  + the selections after it + how history groups it), computed from borrowed state without
  mutating. `EditorState::execute` is the one place that applies a transaction: new text,
  selections, history entry. Cloning the whole state per keystroke (history included) would
  be the purer `state -> state`, but costs O(history) per key for nothing observable.
  Helix's `Transaction` was the inspiration.
- A `ChangeSet` maps positions through an edit (`map_position(pos, Bias)`). Selections,
  decorations, folds, markers and view-state all follow edits with this one function
  (Monaco has a separate "tracked range" mechanism per feature).
- Mutation is allowed where it is an implementation detail with no observable sharing
  (caches: tokenizer states, layout galleys).

## D8. Commands are an enum, not string ids; no context keys

Monaco runs actions by string id (`editor.getAction('editor.action.commentLine')`) and gates
them with `when` clauses on string context keys. Typos fail at run time, arguments are `any`.

Here `Command` is an enum (`Command::MoveLines(Direction::Up)`, `Command::Type(String)`…).
Each command is a function of the state, so "is this command available" is just a function
too (`command.is_available(&state)`), no context-key language. Keybindings map a `KeyChord`
(sequence allowed, for Ctrl+K Ctrl+C) to a `Command`; a `Keymap` is a value an application
can extend or replace. Every command also has a stable `name()` for settings files and
the command palette.

## D9. Language services: traits returning data, called by the application

Monaco: `registerXProvider(languageId, provider)` into a global registry, providers return
promises, the editor decides when to call them.

Here a `LanguageServices` trait with one method per feature (`completions`, `hover`,
`definition`…), each returning plain data and defaulting to "nothing". The editor asks
through the trait; an LSP client is one implementation. Async: requests carry the document
version, results for an older version are dropped. Done after the core (P2).

## D10. Events: the widget reports what happened, no listener soup

Monaco has ~40 `onDidX(listener) -> IDisposable` events and you must dispose each.
Here `CodeEditor::show(ui, &mut state) -> EditorResponse` returns what happened in that
frame: `text_changed`, `selection_changed`, `scrolled`, the `Transaction`s applied, the
mouse target clicked (gutter line, glyph, text position…). The application reacts with
plain `if`s. Nothing to unsubscribe.

## D11. Decorations are typed data, not CSS class names

Monaco decorations carry CSS class names (`className: 'blame-shaded'`) whose look lives in
a style sheet elsewhere; `deltaDecorations(oldIds, new)` makes callers juggle string ids.

Here a `Decoration { range, kind }` where the style is data: background / foreground
colors or theme roles, underline style (squiggle, dotted, solid), whole line, gutter
glyph, line-number color, overview-ruler mark. Callers keep a `DecorationSet` (a value
they own, mapped through edits), and replace it wholesale when they recompute; no ids.

## D12. Options: small typed structs with defaults, no 200-key bag

Monaco's `IEditorOptions` is one flat bag of ~180 optional keys, some taking
`boolean | 'on' | 'off' | 'relative' | function`. Here: `EditorConfig` groups
`Display` (line numbers, whitespace, rulers, guides, wrap…), `Editing` (tab size,
auto-closing, auto-indent…), `Scroll`, `Cursor`, each a struct with `Default`; every
choice is an enum (`LineNumbers::Relative`), no stringly types and no overloaded booleans.

## D13. Syntax highlighting: syntect + two-face grammars, behind a trait

- Diff views must highlight any file. Syntect reads Sublime/TextMate grammars;
  `two-face` bundles bat's ~200 grammars (TypeScript, TOML, Dockerfile… which syntect's
  defaults lack). Pure Rust regexes (`fancy-regex`), no C build.
- Tokenizing is line by line with a saved state per line, like Monaco's tokenizer and
  syntect's own model: an edit re-tokenizes from the edited line until the state
  matches the cached one again.
- Behind a `Tokenizer` trait so tree-sitter can be added (better structure for folding,
  smart select, sticky scroll), the way Zed and Helix do it.
- Rejected: writing a Monarch interpreter first (Monaco's format, but few grammars outside
  Monaco); tree-sitter first (one crate per language, harder to cover "any file" for diffs).

## D14. Diff: `imara-diff` (Myers) for lines, our own refinement inside hunks

- Myers with a line post-processing heuristic is git's default (`diff.algorithm` defaults to
  myers, plus its indent heuristic), so hunks look like `git diff`'s. Histogram was the first
  choice, but imara-diff's histogram took 467 ms on a 100k-line file of distinct lines where
  Myers takes 1.6 ms (`crates/rustext-diff/tests/performance.rs`). Patches don't depend on
  the algorithm: `git apply` takes any valid diff (`tests/git_apply.rs`).
- Inner (char / word) changes are computed per changed hunk with the same library on
  word tokens, then on chars, as Monaco's `charChanges`.
- Results are data (`LineDiff { hunks: Vec<Hunk> }`, each with `inner_changes`), and the
  aligned side-by-side layout, the inline layout and the collapsed unchanged regions are
  pure functions of it. The widget only draws them.

## D15. Rendering text: egui galleys per visible line

Only visible lines are laid out (`LayoutJob` with token colors), egui caches galleys by
content. Columns ↔ x go through the galley, not `column * char_width`, so wide chars,
tabs and ligatures are placed right. Monospace is not assumed in the core.

## D16. Licensing

GPL-3.0-or-later like gitgud, so code can move between the two freely. Easy to relicense
while the author is the only contributor.

## D17. Editors paint display rows, not lines

`CodeEditor` paints a list of display rows (`rustext_core::display::DisplayLayout`): a line of
its text, padding, one collapsed row standing for hidden lines, or a line of a *foreign* text.
Without a layout it is one row per line.

- The diff editor is two plain code editors given aligned layouts (padding where a side has
  no line, collapsed unchanged regions); the inline view is one editor whose removed lines
  are foreign rows from the old text. No second rendering path, no diff-specific widget code
  beyond wiring (Monaco needs view zones for the same).
- Folding and word wrap will be more layouts of the same kind (hidden lines; a line split in
  several rows), Zed's "display map" idea kept to a flat `Vec` of rows: KISS until a 1M-line
  file needs a tree.
- What to show is computed by pure functions (`rustext_diff::display`), tested without UI.

## D18. Partial staging: one rule for every hunk shape

Gitgud's patch builder had five cases (pure insertion, pure deletion, equal and unequal line
counts…), and could not stage a deletion selected in the new side. `rustext_diff::patch` uses
the rule GitHub Desktop uses: for staging, unselected removed lines become context and
unselected added lines are dropped; for reverting (unstage / discard, `apply -R`), the other
way round. `tests/git_apply.rs` applies every subset of changed lines of nine text pairs with
the real `git apply --unidiff-zero` and checks the result.

## D19. No whole-text work per keystroke

Anything computed every frame or every edit reads only the visible lines, a bounded
neighbourhood of the cursor, or the part of a cache an edit touched. Whole-text passes happen
on explicit actions (fold all, replace all, a new diff) or behind a budget (the highlighter).

- Commands report `Outcome::edited_lines` (first line, old last line, new last line per change
  set). Caches keyed by lines shift with `EditedLines::map_line` and recompute only what the
  edit touched (folded blocks, plain-text find matches).
- Fold markers ask `is_fold_header(line)` per visible line; only folded blocks know their
  extent. Indent guides cover the visible lines. `HiddenLines` are disjoint sorted ranges
  (binary search).
- `crates/rustext-egui/tests/performance.rs` (ignored, run in release) measures a 100k-line
  file: idle 0.3 ms, typing 0.3 ms, typing with 20 000 folds 0.5 ms, typing with the find
  widget open on 20 000 matches 4.6 ms, fold all 38 ms. Run it after touching anything
  per-frame; a regression there is a bug.

## D20. Word wrap: continuation rows, column-based breaking

- A wrapped line is one `RowContent::Line(line)` row for its first segment and one
  `RowContent::Wrapped { line, start_column }` row per continuation. Everything that already
  works on rows (folding, padding, scrolling, the gutter) keeps working; a row paints the
  segment of its line from its start column to the next row's.
- Breaking counts visual columns (tabs to tab stops, East Asian wide chars as two), breaking
  after the last whitespace or separator that fits, else inside the word, like Monaco's
  `wordWrapBreakAfterCharacters`. It assumes the editor's monospace font; a proportional
  font would need galley-measured breaks.
- Breaks are cached per line and follow edits through `EditedLines` (D19): only edited lines
  are wrapped again; a width change wraps everything (an explicit, rare event).
- Cursor moves stay per logical line (Up/Down move by line, not by visual row) for now.

## Inspiration (read their code, don't copy it)

| Project | Take |
|---|---|
| **Zed** (`crates/editor`, `crates/text`, gpui) | display map layers: buffer → folds → tabs → wraps → blocks (view zones) → display rows |
| **Helix** (`helix-core`) | `Transaction` / `ChangeSet` / `Selection` as values, `map_pos` with association, ropey |
| **Lapce** (`lapce-core`, floem) | rope deltas, the editor view split from the document |
| **Xi** | rope + delta as the editing primitive |
| **Kakoune** | selections first; anchor/head naming |
| **VS Code / Monaco** | the feature set and keybindings users expect; diff alignment with padding zones |
