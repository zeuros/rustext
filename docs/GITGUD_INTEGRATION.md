# Replacing Monaco in gitgud

Each Monaco use in gitgud (`src/app/components/monaco-editor-view`, `merge-editor`,
`services/file-diff-panel.service.ts`) and what replaces it in rustext. Snippets are what a
Rust gitgud would write; they compile against the current API.

## The diff panel

gitgud: `editor.createDiffEditor(...)`, `setModel({original, modified})`, view types
`split` / `inline` / `hunk` through `renderSideBySide` and `hideUnchangedRegions`.

```rust
use rustext_diff::CollapseOptions;
use rustext_egui::{DiffDocument, DiffEditor, DiffViewConfig, DiffViewMode, EditorTheme, Keymap, ViewConfig};

// Once per file shown (or document.set_texts(old, new) to reuse it).
let mut document = DiffDocument::new(&index_content, &working_tree_content);
document.go_to_change(true); // "the diff opens scrolled to the first change"

// gitgud's view types:
let split = DiffViewConfig { mode: DiffViewMode::SideBySide, ..DiffViewConfig::default() };
let inline = DiffViewConfig { mode: DiffViewMode::Inline, ..DiffViewConfig::default() };
let hunk = DiffViewConfig { collapse_unchanged: Some(CollapseOptions::default()), ..split.clone() };

// Every frame:
let response = DiffEditor::new(&mut document, &split, &view_config, &theme, &keymap)
    .stylers(&mut old_highlighter, &mut new_highlighter)
    .show(ui);
```

- gitgud's options map to `ViewConfig`: `readOnly` is the default for the old side
  (`DiffViewConfig::old_editable` for the other), `minimap: {enabled: false}` is the default
  (`ViewConfig::minimap`), `glyphMargin` is `CodeEditor::glyph_margin`, `renderLineHighlight`
  is `ViewConfig::current_line_highlight`, `colorDecorators` doesn't exist.
- `onDidUpdateDiff` → `response.diff_changed`; `getLineChanges()` → `document.diff().hunks`
  (0-based `LineRange`s, `inner_changes` for the words).
- The `\r` → `␍` hack is gone: a CRLF ↔ LF change is a change in the diff itself, and
  `ViewConfig::show_carriage_returns` marks the `\r` of CRLF lines with a small `CR`.

## Staging, unstaging and discarding lines and hunks

gitgud: `monaco-patch-builder.ts` (`buildZeroContextPatch`, `stageHunk`, `unstageHunk`) then
`git apply --cached --unidiff-zero --allow-overlap -` (stage), `-R --cached` (unstage), `-R`
(discard).

```rust
use rustext_diff::{LineSelection, PatchDirection, build_patch};

// The lines picked in the diff editor (a cursor alone picks its line; inline views can pick
// removed lines too):
let selection = document.selected_lines();
// Or a whole hunk: the one under the cursor.
let hunk_selection = document.hunk_at_cursor().map(|hunk| LineSelection::hunk(document.diff(), hunk));

let direction = PatchDirection::Stage; // PatchDirection::Revert for unstage and discard
if let Some(patch) = build_patch(document.diff(), document.old.text(), document.new.text(), &selection, direction, &file_path) {
    // Same commands as gitgud today:
    // stage:   git apply --cached --unidiff-zero --allow-overlap -
    // unstage: git apply -R --cached --unidiff-zero --allow-overlap -
    // discard: git apply -R --unidiff-zero --allow-overlap -
}
```

One rule covers every hunk shape (DECISIONS D18), including staging a deletion selected in
the new side, which gitgud's builder skips. `crates/rustext-diff/tests/git_apply.rs` applies
every selection of nine text pairs with the real git.

## Context menu on the diff

gitgud: `MonacoDiffRightClickActionsService` adds actions to Monaco's context menu.

```rust
let response = DiffEditor::new(/* … */).show(ui);
response.new.response.context_menu(|ui| {
    if ui.button("Stage selected lines").clicked() { /* build_patch(…, Stage, …) */ ui.close(); }
    if ui.button("Discard selected lines").clicked() { /* build_patch(…, Revert, …) */ ui.close(); }
    if ui.button("Copy file and line reference").clicked() {
        let text = document.new.text();
        let line = text.line_of_offset(document.new.selections().primary().head) + 1;
        ui.ctx().copy_text(format!("{file_path}:{line}"));
        ui.close();
    }
});
// The old side (side-by-side only): response.old.as_ref().map(|old| old.response.context_menu(…)).
```

## Blame

gitgud: a decorations collection with `isWholeLine`, `className: 'blame-shaded'`,
`marginClassName`, and `onMouseDown` on `MouseTarget.GUTTER_LINE_NUMBERS` to select the commit.

```rust
use rustext_core::decoration::{Decoration, DecorationKind};

let shading = Decoration::lines(text, first_line, last_line, DecorationKind::WholeLineBackground(shade));
let summary = Decoration::lines(text, first_line, first_line, DecorationKind::Glyph(color))
    .with_hover_message(format!("{short_sha} {author} · {age}\n{summary}"));

let response = CodeEditor::new(&mut state, &mut view, &view_config, &theme, &keymap)
    .decorations(&blame_decorations)
    .glyph_margin(true)
    .show(ui);
if let Some(rustext_egui::MouseTarget::LineNumber { line }) = response.pressed {
    // select the commit of `line`
}
```

The playground's "Blame demo" (`crates/rustext-demo/src/editor_page.rs`) does exactly this.

### Code lenses (blame summaries, "Stage hunk")

Labels in a row above their line; the click comes back as an index into the slice (D25).

```rust
use rustext_core::services::CodeLens;

let lenses = vec![CodeLens::new(hunk_first_line, "Stage hunk"), CodeLens::new(hunk_first_line, "Discard")];
let response = CodeEditor::new(&mut state, &mut view, &view_config, &theme, &keymap).code_lenses(&lenses).show(ui);
match response.clicked_code_lens.map(|index| lenses[index].label.as_str()) {
    Some("Stage hunk") => { /* build_patch(.., PatchDirection::Stage, ..) */ }
    Some("Discard") => { /* .. */ }
    _ => {}
}
```

## The merge editor

gitgud: mismerge with `git show :1:/:2:/:3:` contents, "Save and mark resolved" (write the
center, `git add`), "keep left / right" (`git checkout --ours/--theirs`).

```rust
use rustext_egui::{MergeDocument, MergeEditor};

let mut document = MergeDocument::new(&base, &ours, &theirs); // stages 1, 2, 3
MergeEditor::new(&mut document, &view_config, &theme, &keymap)
    .labels("Ours: HEAD (main)", "Theirs: feature")
    .stylers([&mut ours_highlighter, &mut theirs_highlighter, &mut result_highlighter])
    .show(ui);
// Save and mark resolved: write document.result_text(), then `git add`.
// document.unresolved_conflict_count() can gate the button.
```

Conflicts follow git's rules (checked against `git merge-file` on 300 generated cases).
`rustext_diff::merge::text_with_conflict_markers` writes git's `<<<<<<<` form when needed.

## Themes

gitgud: `defineTheme('gitgud-light' | 'gitgud-dark', …)` and a global `setTheme`.

```rust
let theme = if dark { EditorTheme::gitgud_dark() } else { EditorTheme::gitgud_light() };
let syntax_theme = if dark { SyntaxTheme::dark_modern() } else { SyntaxTheme::light_modern() };
highlighter.set_theme(syntax_theme); // per highlighter: no global theme
```

## Languages

gitgud: `detectLang(path)` and Monaco's built-in Monarch grammars.

```rust
let languages = LanguageRegistry::new(); // once, share by clone
let language = languages.detect(&path, first_line);
state.language = language.configuration.clone(); // comments, brackets
let highlighter = SyntaxHighlighter::new(&languages, language, syntax_theme);
```

With a language server (`rustext_lsp::LspServices`), go to definition and references can
land in another file: the editor shows one document, so it asks gitgud to open it.

```rust
let response = CodeEditor::new(&mut state, &mut view, &view_config, &theme, &keymap)
    .services(&mut language_server)
    .show(ui);
if let Some(location) = response.open_request {
    // location.document: the file URI (rustext_lsp::client::file_path_of gives its path);
    // location.range: in chars. Open it, then select the range in its editor.
}
```

## Still to do on the gitgud side

- Binary and image files: keep gitgud's checks (NUL in the first 8000 bytes, size limits)
  before creating a `DiffDocument`.
- Undo of git actions, file watching, and the rest of gitgud are unrelated to the editor.
- A "multi-file diff" (Monaco's `createMultiFileDiffEditor`) is not implemented; gitgud
  shows one file at a time today.
