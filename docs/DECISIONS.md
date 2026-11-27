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
- Since D22, TextMate grammars only highlight the languages without a bundled tree-sitter
  grammar (`textmate.rs`).

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
  file: idle 0.4 ms, typing 0.5 ms (at the end of the file too), typing with 20 000
  folds 0.7 ms, typing with the find widget open on 20 000 matches 4.7 ms, fold all 37 ms,
  Ctrl+End 12 ms (the bracket line states of 10 000 lines, the per-frame budget; the next
  frames average 3.6 ms while the states of the lines above catch up). Highlighted
  with tree-sitter (D22): typing 1.6 ms (fold markers from the tree included), at the end
  5.4 ms, Ctrl+End 15 ms; the first parse
  (~450 ms) runs on its thread while frames take 0.4 ms. Run it after touching anything
  per-frame; a regression there is a bug. kittest keeps AccessKit on, so these include the
  screen reader text runs (~0.15 ms a frame), which an application without a screen reader
  doesn't build.

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
- Diffs wrap too (Monaco's `diffWordWrap: 'inherit'`): each side's editor wraps its lines
  under the layout it is given (`wrap_under_given_layout`), and the diff editor lays the
  rows out from those points the next frame: an aligned pair of lines takes the rows of the
  longer one, the other side padded. Removed lines of an inline view are the old text's,
  painted as foreign rows: they don't wrap.
- Up/Down move by visual row when wrapping, as in VS Code: the widget handles them (it knows
  the rows), keeping the visual column within the row in `Selection::preferred_visual_column`.
  The core's line-based vertical motion stays for unwrapped views.

## D21. Brackets in strings and comments: a lexer from the language configuration

Bracket colors and matching ignore brackets inside strings and comments, as Monaco does, but
without asking the highlighter (which is budgeted, lazy and behind `LineStyler`): a small
lexer (`rustext_core::lexical`) reads the comment markers and quotes of the
`LanguageConfiguration`, line by line, carrying a `LexicalContext` (code, block comment,
string) from one line to the next.

- `string_quotes` open strings that may span lines (Rust, Python's `"""`, JS templates);
  `char_quote` only opens a char literal when one follows (`'x'`, `'\n'`), so a Rust
  lifetime `'a` stays code. Plain text has no strings (an apostrophe in prose opens nothing).
- `BracketLineStarts` caches the depth and context at each line start, follows edits like
  the highlighter (D19) and is shared by colorization and matching, so matching backwards
  reads the states of earlier lines instead of guessing.
- Accepted: raw strings (`r#"..."#`), nested comments and heredocs are not understood.
  Where a tree-sitter grammar is bundled, its tokens replace the lexer (D27).

## D22. Tree-sitter where a grammar is bundled, TextMate grammars elsewhere

`SyntaxHighlighter` stays the one type applications use; it parses with tree-sitter when the
language has a bundled grammar (18 crates: Rust, Python, JS, TS/TSX, JSON, Go, C, C++, Bash,
HTML, CSS, TOML, YAML, Java, C#, Ruby, Lua) and falls back to syntect (D13) for the ~200
others, so diffs of any file stay highlighted.

- Highlighting runs the grammar crate's `highlights.scm` over the lines being painted, in
  blocks of 32 lines (one query per block, not per line). A smaller node wins over the node
  holding it; of two patterns capturing one node, the first wins (tree-sitter-highlight's
  rule). Capture names map to TextMate scopes (`function.method` → `entity.name.function`),
  so every theme, built-in or `.tmTheme`, keeps working.
- Parsing runs on a thread per highlighter: an incremental parse of a 3 MB Rust file takes
  ~11 ms whatever the edit (measured in `crates/rustext-syntax/tests/performance.rs`), too
  much for every keystroke's frame (D19). Until the new tree arrives, lines are highlighted
  from the previous tree edited to match the text (`Tree::edit` shifts its nodes); edits made
  while a parse runs are replayed on its tree. A frame waits up to 3 ms for a parse whose
  previous run fit in that time, so small files are highlighted in the frame that edited them.
- `LineStyler::text_edited` hands the styler each change set with the text before it, so it
  can edit its tree; `LineStyler::enclosing_syntax_node` gives smart select the node holding
  a selection. Both have defaults, so other stylers ignore them.
- Folding (`FoldingStrategy::Auto`, the default) asks the tree for the fold a line starts:
  a bracketed node (`{…}`, `[…]`, `(…)`) folds from its opening line; a node ending with a
  bracket doesn't (`if {…} else {…}` folds through its two blocks, as rust-analyzer's folding
  ranges do); another multi-line node (Python `def`, Ruby `def…end`, a comment, an HTML
  element) folds unless it is the last named child of a construct started earlier (Python's
  `block`). The last line stays visible when it only closes (`}`, `end`, `</div>`).
  Without a grammar, folding is by indentation, as before.
- Go to Symbol (Ctrl+Shift+O) asks the services first (an LSP server knows more), and runs
  the grammar crate's `tags.scm` when they have nothing: `@definition.*` captures named by
  `@name`. The command palette and the symbol list are one fuzzy list (`PaletteMode`).
- Queries come from the grammar crates (MIT). Helix's and nvim-treesitter's are richer but
  MPL/Apache files with their own conventions (`#set!`, `inherits`); vendoring them is a later
  choice. Injections (JS in HTML, code in Markdown) aren't run yet.
- Rejected: `tree-house` (Helix's highlighter): it loads grammars as shared libraries at run
  time and has its own bindings; the published grammar crates link statically with the
  official `tree-sitter` crate. `tree-sitter-highlight`: it highlights a whole `&[u8]`, not
  a range of a rope.

## D23. LSP messages are `lsp-types` values

The client builds and reads `lsp-types` structs instead of hand-written JSON: one generic
`ask::<R: Request>(text, key, R::Params) -> Reply<R::Result>` sends any request and parses its
answer, keyed by `R::METHOD` and the text revision and position asked about. A result that
doesn't parse counts as empty, like a server that has nothing to say.

- The protocol's shapes (`CompletionResponse` as a list or an array, three `HoverContents`,
  `GotoDefinitionResponse` links, `documentChanges` operations) are enums the compiler
  checks, not `value["key"]` lookups that return null on a typo.
- The fake server stays hand-written JSON on purpose: it checks the wire format independently
  of the types the client uses. `tests/rust_analyzer.rs` (ignored, needs `rust-analyzer`)
  checks that a real server's answers parse.
- Rejected: `async-lsp`/`tower-lsp` (async runtimes for servers; the client is poll-based, D9).

## D24. Fuzzy matching with `nucleo`, search with `regex-cursor`

Both come from Helix's authors and are published as standalone crates.

- `FuzzyQuery` wraps a `nucleo_matcher` atom and its matcher, so one query scores many
  candidates (completions, palette entries) without allocating. Case is ignored, prefixes
  are preferred; equal scores put the shorter label first.
- `find_matches` runs a `regex-cursor` regex over the rope's chunks: nothing is copied, and
  match offsets come back as bytes of the rope, converted to chars by the rope in O(log n).
  The `regex` crate stays for expanding `$1` groups in one match's replacement, from the same
  pattern (`SearchQuery::regex_pattern`).

## D25. Code lenses are rows the application fills, clicks are reported

`CodeEditor::code_lenses(&[CodeLens { line, label }])` puts a row above each line with
lenses, its labels aligned with the line's indentation. A click sets
`EditorResponse::clicked_code_lens` to the lens's index in that slice; the application runs
what it means (gitgud: stage a hunk, show a commit). No command ids, no resolve step: Monaco's
`CodeLensProvider` returns command ids to look up in a global registry (D8).

- The lens row is `RowContent::CodeLens(line)` in the editor's own layout (folds, wraps), so
  scrolling, clicks below it and hidden lines work as for any row; a layout an application
  passes (diffs) has no lens rows.
- Lenses come every frame from the caller (like decorations); keeping them on the right line
  across edits is the caller's job (recompute, or map through `Outcome::applied_changes`).
- The language services' lenses (`LanguageServices::code_lenses`, LSP `codeLens` then
  `codeLens/resolve` for each lens without a command) show after the application's, asked
  once the text rested 0.3 s (not on every keystroke); until the next answer they follow
  their line's text through edits. Their click is a `CodeLensAction`: `ShowLocations`
  opens the references list ("3 implementations": a command whose third argument is a list
  of locations, as VS Code's `editor.action.showReferences`), `Run(ServiceCommand)` is
  handed to the application as `EditorResponse::service_command` (a name and JSON
  arguments: it alone can run a test or a debugger). The LSP client announces the
  client-side commands the editor runs itself (`rust-analyzer.showReferences`), as
  rust-analyzer only sends the lenses whose command the client can run.

## D26. Other documents: the services name them, the application opens them

`definition` and `references` answer `Location { document: Option<String>, range }`:
`None` is the edited document, `Some` another one, named as the services name it (the LSP
client: its file URI). The editor shows one document, so going to another one ends as
`EditorResponse::open_request`; the application opens it (gitgud: in its file view).

- Ranges are in chars in every document (D5): the LSP client reads another file once per
  answer to convert its UTF-16 columns, and keeps the server's columns when it can't.
- F12 and Ctrl+click are requests answered on a later frame, like Find References: a real
  server rarely answers in the frame that asked. An edit before the answer drops it.

## D27. Brackets from the syntax tree's tokens, the lexer meanwhile

Where the styler parses the text (D22), a bracket is code when it is part of a bracket token
of the tree: `LineStyler::code_brackets` answers the columns of a line's bracket tokens, and
`BracketScanner` turns them into the same `LineScan` the lexer (D21) gives, so depths,
colors, matching and the bracket pair guide are unchanged and need no tree of their own.

- Bracket tokens are found once per grammar: its anonymous node kinds of at most three
  punctuation chars holding a bracket (`(`, `${`, `$((`, `[[`), queried in aligned blocks of
  256 lines cached until the tree changes. Brackets inside strings, raw strings, nested
  comments, heredocs and markup text belong to other tokens and are not captured.
- Only a tree matching the text answers (`SyntaxBrackets::Parsing` otherwise): an edited tree
  knows nothing of a bracket just typed. Meanwhile the lexer answers, and
  `BracketLineStarts` remembers the first line it scanned so; once the tree is parsed, the
  states after that line are computed again from it.
- A grammar without bracket tokens (HTML) has its `text` nodes queried instead: brackets of
  prose pair, those of attribute values don't; its scripts' come from their own tree (D29).
  Matching still counts brackets
  rather than following tree nodes, as Monaco does: it stays the same for both sources and
  works in broken code.
- Cost: steady typing is unchanged; the bracket states of a whole 100k-line file (after
  Ctrl+End) take ~25 ms frames for ten frames instead of ~16 ms.

## D28. Quality gates are Rust-native tools, no Sonar

CI fails on: rustfmt, clippy `-D warnings`, unused dependencies (`cargo shear`), source files
over 500 lines before their test module (`scripts/file_size.sh`), and tests (nextest, plus
doctests) on Linux, Windows and macOS. Weekly, `cargo mutants` runs on the pure crates (core,
diff, syntax) and uploads the mutants no test kills. Renovate opens grouped dependency updates.

- Files over the limit are pinned in `scripts/file_size_baseline.txt` at their current size:
  they may shrink, never grow, and leave the list once they fit. Splitting one means running
  `scripts/file_size.sh --baseline`.
- The complexity lints (`clippy.toml`: 60 lines, 5 arguments, nesting 5, complexity 15, 3
  bools) are pinned the same way: each function or struct over them when they came carries
  `#[expect(clippy::…, reason = "predates the complexity lints (D28)")]`. New code meets them;
  an `expect` fails once its item is brought under the limit, and is then removed.
- Sonar's Rust analysis mostly re-reads clippy's; its dashboards don't pay for a second
  service. Randomized tests (`random_commands`, `random_diffs`, `random_events`) stay
  hand-rolled and seeded; proptest would only add shrinking.

## D29. Embedded languages are layers parsed over the host's parts

A tree-sitter grammar may come with an injection query (HTML's: `<script>` is JavaScript,
`<style>` CSS). The background parser parses the text, runs that query on the tree, then
parses each embedded language over the captured parts only (`Parser::set_included_ranges`),
all the parts of one language in one tree, as Helix's combined injections: the scripts of a
page share their names, and one tree per language reuses its previous tree incrementally.

- `TreeHighlighter` holds layers: the text's own (first) and one per embedded language with a
  bundled grammar (`registry.grammar`); edits are applied to every tree.
- What is read from a tree is cut to the byte ranges it was parsed over
  (`Tree::included_ranges`), so a node of a combined tree spanning two scripts never colors
  the markup between them. Of two captures, an embedded language wins over the host.
- Brackets, symbols (Go to Symbol finds a script's functions), folds and smart select read
  every layer: smart select takes the smallest node of any layer, folds the one reaching
  furthest.
- One level only: an embedded language's own injections (JS regexes, JSDoc) are not followed.
- Markdown, Vue and Svelte stay with their TextMate grammars, which embed fenced code and
  scripts already (D13).

## Inspiration (read their code, don't copy it)

| Project | Take |
|---|---|
| **Zed** (`crates/editor`, `crates/text`, gpui) | display map layers: buffer → folds → tabs → wraps → blocks (view zones) → display rows |
| **Helix** (`helix-core`) | `Transaction` / `ChangeSet` / `Selection` as values, `map_pos` with association, ropey |
| **Lapce** (`lapce-core`, floem) | rope deltas, the editor view split from the document |
| **Xi** | rope + delta as the editing primitive |
| **Kakoune** | selections first; anchor/head naming |
| **VS Code / Monaco** | the feature set and keybindings users expect; diff alignment with padding zones |
