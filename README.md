# rustext

A code editor and diff editor in Rust, with Monaco's feature set and without its design
mistakes. Built to replace Monaco in [gitgud](https://github.com/zeuros/gitgud) when it moves
to full Rust.

```
cargo run -p rustext-demo --release                 # the playground
cargo run -p rustext-demo --release -- some/file.rs # open a file
cargo run -p rustext-demo --release -- --diff old.rs new.rs
```

## What works

**Editing**: multi-cursor (Ctrl+D, Ctrl+Shift+L, Alt+click, Ctrl+Alt+↑/↓, Shift+Alt+drag box
selection), word / word-part / smart-Home motions over grapheme clusters, auto-closing and
overtyping brackets and quotes, surround, auto-indent, Tab / Shift+Tab, line moves and copies,
delete / join / sort / reverse / dedupe lines, comments (line and block), case transforms,
undo / redo with typing groups and cursor restore, whole-line copy / paste, paste spread over
cursors, IME, find / replace (regex, groups, case, whole word, preserve case), go to line,
indentation folding, bracket matching, occurrence highlights, indent guides, whitespace
rendering, VS Code keybindings including chords (Ctrl+K Ctrl+…).

**Viewing**: syntax highlighting for ~200 languages (bat's grammars through syntect),
VS Code-like Dark / Light Modern themes and gitgud's, decorations (backgrounds, borders,
squiggles, gutter glyphs, line-number colors, overview marks) that follow edits.

**Diffs**: side-by-side and inline diff editor with aligned rows, word-level changes, collapsed
unchanged regions, F7 navigation, CRLF and missing-newline changes made visible, and patches
of selected lines for `git apply` (stage, unstage, discard), checked against the real git.

## Layout

| Crate | |
|---|---|
| `rustext-core` | text model, selections, edits, history, commands — no UI |
| `rustext-syntax` | languages, token themes, incremental highlighting |
| `rustext-diff` | line diff, inner changes, row layouts, patches |
| `rustext-egui` | `CodeEditor` and `DiffEditor` widgets |
| `rustext-demo` | the playground |

## Docs

- [docs/PLAN.md](docs/PLAN.md): milestones and where work resumes
- [docs/DECISIONS.md](docs/DECISIONS.md): design choices, and the Monaco designs avoided
- [docs/MONACO_FEATURES.md](docs/MONACO_FEATURES.md): every Monaco feature, its API and status
- [CLAUDE.md](CLAUDE.md): conventions and commands for contributors (human or not)

License: GPL-3.0-or-later.
