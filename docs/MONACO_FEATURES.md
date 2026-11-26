# Monaco feature inventory

Source: `monaco-editor` 0.58 typings (`monaco.d.ts`, read from `../gitgud/node_modules`), the
built-in action IDs in `esm/vs/editor/contrib`, and the official playground's sample list.

Each feature lists the Monaco API it comes from, then its priority here:

- **P0**: gitgud uses it today (see "What gitgud uses"), needed to replace Monaco there.
- **P1**: what makes Monaco feel like Monaco while editing; done right after P0.
- **P2**: language-service surface (providers), done once the core is solid.
- **P3**: nice to have, or web-specific; maybe never.

`Status` is updated as milestones land (`done` can still have gaps, listed in PLAN.md) (see [PLAN.md](PLAN.md)): `todo`, `wip`, `done`, `skip`.

## What gitgud uses

From `../gitgud/src/app/components/monaco-editor-view`, `merge-editor` and `file-diff-panel.service.ts`:

| Monaco API | gitgud use |
|---|---|
| `editor.createDiffEditor`, `setModel({original, modified})` | the diff panel |
| `renderSideBySide`, `hideUnchangedRegions` | split / inline / hunk views |
| `onDidUpdateDiff`, `getLineChanges()` | hunks for stage / unstage / discard of a hunk, a line, a selection of lines (`monaco-patch-builder.ts`) |
| `getOriginalEditor()`, `getModifiedEditor()`, selections | which lines are selected on each side |
| `editor.create` + `createDecorationsCollection` (`isWholeLine`, `className`, `marginClassName`) | blame view: shaded line groups |
| `onMouseDown` with `MouseTargetType.GUTTER_LINE_NUMBERS` | blame: click a line number to select its commit |
| `defineTheme` / `setTheme` (colors `editor.background`, `diffEditor.insertedTextBackground`…) | app themes |
| `readOnly`, `minimap: {enabled: false}`, `glyphMargin`, `renderLineHighlight`, `colorDecorators` | options |
| context menu actions on the diff | stage / discard hunk or lines, copy file + line reference |
| Monarch tokenizers (built-in languages) | syntax highlighting of any file |
| `renderControlCharacters` + the `\r` → `␍` hack | making CRLF visible |
| merge editor (3 models) | conflict resolution, accept a side |

## 1. Text model (`editor.ITextModel`)

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Text storage, line access | `getValue`, `getLineContent`, `getLineCount`, `getLinesContent`, `getValueInRange` | P0 | done |
| Offset ↔ position | `getOffsetAt`, `getPositionAt`, `validatePosition`, `validateRange`, `modifyPosition` | P0 | done |
| End of line handling | `getEOL`, `setEOL`, `pushEOL`, `EndOfLineSequence`, `DefaultEndOfLine` | P0 | done |
| Edits | `applyEdits`, `pushEditOperations` (with cursor computer) | P0 | done |
| Undo / redo with stops | `undo`, `redo`, `canUndo`, `pushStackElement`, `popStackElement` | P1 | done |
| Version ids | `getVersionId`, `getAlternativeVersionId` (dirty tracking across undo) | P1 | done |
| Find | `findMatches`, `findNextMatch`, `findPreviousMatch` (regex, case, whole word, in range) | P1 | done |
| Words | `getWordAtPosition`, `getWordUntilPosition`, `wordSeparators` | P1 | done |
| Indentation | `detectIndentation`, `normalizeIndentation`, `tabSize`, `insertSpaces`, `indentSize` | P1 | done |
| Whitespace columns | `getLineFirstNonWhitespaceColumn`, `getLineLastNonWhitespaceColumn` | P1 | done |
| Snapshots | `createSnapshot` (stream the text) | P3 | todo |
| Model registry by URI | `createModel(value, lang, uri)`, `getModel(uri)`, `getModels` | — | skip: see DECISIONS D6 |

## 2. Cursors and selections

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Single cursor, selection | `setPosition`, `setSelection`, `getSelection`, `onDidChangeCursorPosition` | P0 | done |
| Multi-cursor | `setSelections`, `getSelections`, `multiCursorModifier`, `multiCursorMergeOverlapping`, `multiCursorLimit` | P1 | done |
| Add cursor above / below | `editor.action.insertCursorAbove/Below` (Ctrl+Alt+↑/↓) | P1 | done |
| Add next occurrence | `editor.action.addSelectionToNextFindMatch` (Ctrl+D), `moveSelectionToNextFindMatch` (Ctrl+K Ctrl+D), `selectHighlights` (Ctrl+Shift+L) | P1 | done |
| Cursors at line ends | `insertCursorAtEndOfEachLineSelected` (Shift+Alt+I) | P1 | done |
| Column (box) selection | `columnSelection`, Shift+Alt+drag, `cursorColumnSelect*` | P1 | done |
| Cursor motions | `cursorLeft/Right/Up/Down/Home/End/Top/Bottom/PageUp/PageDown` (+`Select`) | P0 | done |
| Word motions | `cursorWordLeft/Right`, `cursorWordStart*/End*`, `cursorWordPart*` (camelCase parts) | P1 | done |
| Smart Home (first non-whitespace, then column 1) | `cursorHome` | P1 | done |
| Sticky column on vertical moves | implicit | P0 | done |
| Cursor undo | `cursorUndo` / `cursorRedo` (Ctrl+U) | P2 | todo |
| Selection anchor | `setSelectionAnchor`, `selectFromAnchorToCursor` | P3 | todo |
| Smart select expand / shrink | `editor.action.smartSelect.expand/shrink` (Shift+Alt+→/←) | P2 | todo |
| Cursor style, blinking, width, smooth caret | `cursorStyle`, `cursorBlinking`, `cursorWidth`, `cursorSmoothCaretAnimation` | P1 | done |
| Cursor surrounding lines | `cursorSurroundingLines` | P2 | done (`ViewConfig::cursor_surrounding_lines`) |

## 3. Editing behaviour

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Typing, backspace, delete, enter | type / deleteLeft / deleteRight | P0 | done |
| Auto-indent on Enter | `autoIndent`, `onEnterRules`, `indentationRules` | P1 | done |
| Auto-closing pairs | `autoClosingBrackets`, `autoClosingQuotes`, `autoClosingDelete`, `autoClosingOvertype`, language `autoClosingPairs` | P1 | done |
| Surround selection | `autoSurround`, language `surroundingPairs` | P1 | done |
| Tab / Shift+Tab indent / outdent | `editor.action.indentLines`, `outdentLines`, `useTabStops`, `tabFocusMode` | P1 | done |
| Line comment / block comment | `editor.action.commentLine` (Ctrl+/), `blockComment` (Shift+Alt+A), language `comments` | P1 | done |
| Move lines up / down | `moveLinesUpAction` (Alt+↑/↓) | P1 | done |
| Copy lines up / down | `copyLinesUpAction` (Shift+Alt+↑/↓) | P1 | done |
| Delete line | `deleteLines` (Ctrl+Shift+K) | P1 | done |
| Duplicate selection | `duplicateSelection` | P1 | done |
| Join lines | `joinLines` | P1 | done |
| Sort lines, reverse, remove duplicates | `sortLinesAscending/Descending`, `reverseLines`, `removeDuplicateLines` | P1 | done |
| Transpose letters | `transposeLetters` | P2 | todo |
| Case transforms | `transformToUppercase/Lowercase/Titlecase/Snakecase/Camelcase/Kebabcase/Pascalcase` | P1 | done |
| Trim trailing whitespace | `editor.action.trimTrailingWhitespace` | P1 | done |
| Insert final newline | contrib `insertFinalNewLine` | P2 | todo |
| Clipboard | `clipboardCopyAction`, `Cut`, `Paste`, `emptySelectionClipboard` (copy whole line), `multiCursorPaste` (spread), `copyWithSyntaxHighlighting` | P0 | done |
| Overtype mode (Insert key) | `overtypeCursorStyle`, `overtypeOnPaste` | P2 | todo |
| In-place replace (cycle true/false…) | `inPlaceReplace.up/down` | P3 | todo |
| Read-only + message | `readOnly`, `readOnlyMessage`, `onDidAttemptReadOnlyEdit` | P0 | done |
| IME composition | `onDidCompositionStart/End`, `inComposition` | P0 | done |
| Linked editing (HTML tag pairs) | `linkedEditing`, `registerLinkedEditingRangeProvider` | P2 | todo |
| Snippets with tab stops | `snippetSuggestions`, `insertTextRules.InsertAsSnippet` | P2 | done |
| Drag and drop text | `dragAndDrop`, `dropIntoEditor` | P3 | todo |

## 4. Rendering

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Line numbers: on, off, relative, interval, custom | `lineNumbers` (`RenderLineNumbersType`), `lineNumbersMinChars` | P0 | done |
| Current line highlight | `renderLineHighlight` (`line`, `gutter`, `all`), `renderLineHighlightOnlyWhenFocus` | P0 | done |
| Selection painting, rounded | `roundedSelection` | P0 | done |
| Whitespace rendering | `renderWhitespace` (`none`, `boundary`, `selection`, `trailing`, `all`) | P1 | done |
| Control characters | `renderControlCharacters` (and visible CR, the gitgud hack done right) | P0 | done |
| Rulers | `rulers` (columns, colors) | P1 | done |
| Indent guides, active guide | `guides.indentation`, `guides.highlightActiveIndentation` | P1 | done |
| Bracket pair colorization | `bracketPairColorization.enabled`, `independentColorPoolPerBracketType` | P1 | done (skips strings and comments, D21) |
| Bracket pair guides | `guides.bracketPairs`, `bracketPairsHorizontal` | P2 | todo |
| Matching bracket highlight, jump | `matchBrackets`, `editor.action.jumpToBracket` (Ctrl+Shift+\), `selectToBracket` | P1 | done (skips strings and comments, D21) |
| Selection occurrences highlight | `selectionHighlight` | P1 | done |
| Word occurrences highlight | `occurrencesHighlight` (`singleFile`, `multiFile`), `wordHighlight.next/prev` | P1 | done |
| Word wrap | `wordWrap` (`off`, `on`, `wordWrapColumn`, `bounded`), `wrappingIndent`, `wordWrapBreakBeforeCharacters` | P1 | done |
| Scrolling | `scrollBeyondLastLine`, `scrollBeyondLastColumn`, `smoothScrolling`, `mouseWheelScrollSensitivity`, `fastScrollSensitivity` (Alt), `scrollPredominantAxis` | P0 | done |
| Scrollbars with overview ruler | `scrollbar.*`, `overviewRulerLanes`, `overviewRulerBorder`, `hideCursorInOverviewRuler` | P0 | done |
| Minimap | `minimap.enabled/side/size/scale/renderCharacters/showSlider/autohide`, section headers (`// MARK:`) | P2 | done |
| Sticky scroll (scopes pinned at the top) | `stickyScroll.enabled/maxLineCount/defaultModel` | P2 | done |
| Padding top / bottom | `padding` | P1 | done |
| Fonts, ligatures, line height, letter spacing, zoom | `fontFamily`, `fontSize`, `fontLigatures`, `lineHeight`, `letterSpacing`, `mouseWheelZoom`, `fontZoomIn/Out/Reset` | P0 | done |
| Unicode highlighting (invisible, ambiguous chars) | `unicodeHighlight.*` | P2 | todo |
| Unusual line terminators (LS, PS, NEL) | `unusualLineTerminators` | P2 | done: lines split on `\n` only, others are chars (D4) |
| Long lines: stop rendering after N chars | `stopRenderingLineAfter` | P1 | done |
| Placeholder text when empty | `placeholder` | P2 | done (`ViewConfig::placeholder`) |
| Final newline rendering | `renderFinalNewline` | P2 | todo |
| Variable line heights / fonts | `allowVariableLineHeights`, decoration `lineHeight` | P3 | todo |

## 5. Decorations and widgets

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Range / inline decorations | `createDecorationsCollection`, `deltaDecorations`, `IModelDecorationOptions.className/inlineClassName` | P0 | done |
| Whole-line decorations | `isWholeLine`, `marginClassName`, `lineNumberClassName` | P0 | done |
| Glyph margin icons | `glyphMargin`, `glyphMarginClassName`, `GlyphMarginLane`, `addGlyphMarginWidget` | P1 | done |
| Line decorations (left of the text, e.g. git gutter bars) | `linesDecorationsClassName`, `lineDecorationsWidth` | P1 | done (`DecorationKind::GutterBar`) |
| Overview ruler marks | `overviewRuler: {color, position}` | P0 | done |
| Minimap marks | `minimap: {color, position}` | P2 | todo |
| Injected text (before / after content) | `before`, `after` (`InjectedTextOptions`) | P2 | done |
| Decoration hover message | `hoverMessage`, `glyphMarginHoverMessage` | P2 | done |
| Stickiness when editing at the edges | `TrackedRangeStickiness` | P0 | done |
| View zones (space between lines, e.g. diff padding, inline widgets) | `changeViewZones`, `IViewZone` | P0 (diff alignment) | done |
| Content widgets (anchored at a position) | `addContentWidget` | P2 | skip: an egui `Area` at `EditorGeometry::cell_center` |
| Overlay widgets (fixed in the editor, e.g. find) | `addOverlayWidget` | P1 | skip: an egui `Area` over the editor's rect |
| Markers (diagnostics, squiggles) | `setModelMarkers`, `IMarkerData` (severity, tags `Unnecessary`, `Deprecated`), `showUnused`, `showDeprecated` | P1 | done |
| Go to next problem | `editor.action.marker.next` (F8) | P2 | done |
| Banner | `setBanner` | P3 | todo |

## 6. Find and replace (`contrib/find`)

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Find widget (Ctrl+F), next / previous (Enter, F3) | `actions.find`, `find.*` options | P0 | done |
| Case, whole word, regex toggles | Alt+C, Alt+W, Alt+R | P1 | done |
| Replace, replace all, regex groups `$1` | `editor.action.startFindReplaceAction` (Ctrl+H) | P1 | done |
| Preserve case in replace | Alt+P | P2 | done |
| Find in selection | `find.autoFindInSelection` | P1 | done (toggle, and automatic for multi-line selections) |
| Seed from selection / word | `find.seedSearchStringFromSelection` | P1 | done |
| Match count "3 of 17", highlights in overview ruler | — | P1 | done |
| Select all matches as cursors | Alt+Enter | P1 | done |

## 7. Folding (`contrib/folding`)

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Indentation-based folding | `foldingStrategy: 'indentation'` | P1 | done |
| Language / provider folding | `registerFoldingRangeProvider`, `foldingStrategy: 'auto'` | P2 | todo |
| Marker regions (`#region` / `// region`) | language `folding.markers`, `foldAllMarkerRegions` | P2 | todo |
| Fold / unfold / toggle, recursive, all, level N | `editor.fold`, `unfold`, `toggleFold`, `foldAll`, `unfoldAll`, `foldLevel1..7`, `foldRecursively` | P1 | done |
| Fold controls in the gutter | `showFoldingControls` (`always`, `mouseover`, `never`) | P1 | done |
| Folded region highlight, `…` after the line | `foldingHighlight` | P1 | done |
| Unfold on click after end of line | `unfoldOnClickAfterEndOfLine` | P2 | todo |
| Hidden areas (generic, used by diff hunk view) | `onDidChangeHiddenAreas`, `setHiddenAreas` | P0 | done |

## 8. Syntax and languages (`languages.*`)

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Syntax highlighting of many languages | Monarch (`setMonarchTokensProvider`), ~80 basic languages | P0 | done |
| Language configuration | `setLanguageConfiguration` (comments, brackets, autoClosingPairs, surroundingPairs, folding, wordPattern, indentationRules, onEnterRules) | P1 | done |
| Language registry and detection | `register`, `getLanguages`, `onLanguage` (by extension, filename, first line) | P0 | done |
| Custom tokenizer | `setTokensProvider` (`IState`, line-by-line) | P2 | todo |
| Semantic tokens | `registerDocumentSemanticTokensProvider` | P3 | todo |
| Standalone colorize (to HTML) | `colorize`, `colorizeElement`, `colorizeModelLine`, `tokenize` | P2 | todo |
| Inspect tokens (debug) | `editor.action.inspectTokens` | P2 | todo |

## 9. Language services (providers)

All `languages.register*Provider`. Design here: see DECISIONS D9.

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Completion (suggest widget, fuzzy filter, details, commit chars, snippets) | `registerCompletionItemProvider`, `quickSuggestions`, `suggest.*`, `acceptSuggestionOnEnter`, `tabCompletion`, `wordBasedSuggestions` | P2 | done |
| Hover | `registerHoverProvider`, `hover.delay/sticky/above` | P2 | done |
| Signature help | `registerSignatureHelpProvider`, `parameterHints` | P2 | done |
| Go to definition / declaration / type / implementation, Ctrl+click | `register*DefinitionProvider`, `gotoLocation.*`, `definitionLinkOpensInPeek` | P2 | partial: definition in the same text (F12, Ctrl+click) |
| References, peek view | `registerReferenceProvider`, `peekView` | P2 | todo |
| Document highlights | `registerDocumentHighlightProvider` | P2 | done (replaces the word highlight when the services answer; LSP too) |
| Rename (F2) | `registerRenameProvider`, `registerNewSymbolNameProvider` | P2 | done (same text) |
| Code actions, lightbulb, quick fix | `registerCodeActionProvider`, `lightbulb.enabled` | P2 | todo |
| Code lens | `registerCodeLensProvider`, `codeLens` | P2 | todo |
| Formatting (document, range, on type, on paste) | `registerDocumentFormattingEditProvider`, `…Range…`, `…OnType…`, `formatOnType`, `formatOnPaste` | P2 | todo |
| Inlay hints | `registerInlayHintsProvider`, `inlayHints.*` | P2 | done: `LanguageServices::inlay_hints` per block of visible lines, LSP `textDocument/inlayHint`; hints aren't clickable |
| Inline completions (ghost text, AI) | `registerInlineCompletionsProvider`, `inlineSuggest.*` | P2 | done (`inline_completion`) |
| Links (Ctrl+click URLs) | `registerLinkProvider`, `links`, `registerLinkOpener` | P1 | todo |
| Color decorators and picker | `registerColorProvider`, `colorDecorators` | P3 | todo |
| Document symbols, outline, Go to symbol (Ctrl+Shift+O) | `registerDocumentSymbolProvider`, quick access `@` | P2 | todo |
| Selection ranges (smart select) | `registerSelectionRangeProvider` | P2 | todo |
| LSP bridge | not in Monaco (monaco-languageclient) | P2 | done |

## 10. Commands, keybindings, menus

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Built-in commands (~150 ids listed above) | `editor.getAction(id).run()`, `trigger(source, id)` | P0 | done |
| Custom actions with keybinding + context menu entry | `addAction({id, label, keybindings, contextMenuGroupId, precondition, run})` | P0 | done |
| Keybinding rules, chords (Ctrl+K Ctrl+C) | `addKeybindingRule(s)`, `KeyMod`, `KeyCode` | P1 | done |
| Context keys (`when` clauses) | `createContextKey`, `precondition` | — | skip: see DECISIONS D8 |
| Context menu | `contextmenu` | P0 | done |
| Command palette (F1) | quick access `>` | P2 | done |
| Go to line (Ctrl+G) | quick access `:` | P1 | done |

## 11. Diff editor (`IDiffEditor`) — gitgud's main use

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Line diff + char/word-level inner changes | `getLineChanges()` (`ILineChange.charChanges`), `diffAlgorithm` (`legacy`, `advanced`) | P0 | done |
| Side-by-side, aligned with padding zones | `renderSideBySide: true` | P0 | done |
| Inline (unified) view | `renderSideBySide: false` | P0 | done |
| Auto inline when narrow | `useInlineViewWhenSpaceIsLimited`, `renderSideBySideInlineBreakpoint` | P1 | done |
| Collapse unchanged regions, expand on click | `hideUnchangedRegions.{enabled, contextLineCount, minimumLineCount, revealLineCount}` | P0 | done |
| Ignore trim whitespace | `ignoreTrimWhitespace` | P0 | done |
| +/- indicators, revert arrows in the gutter | `renderIndicators`, `renderMarginRevertIcon`, `renderGutterMenu` | P1 | done |
| Overview ruler of changes | `renderOverviewRuler` | P0 | done |
| Resizable split | `enableSplitViewResizing`, `splitViewDefaultRatio` | P1 | done |
| Navigate changes (F7 / Shift+F7) | `goToDiff('next'|'previous')`, `revealFirstDiff` | P0 | done |
| Original side editable | `originalEditable` | P2 | done (`DiffViewConfig::old_editable`) |
| Moved code detection | `experimental.showMoves` | P2 | todo |
| Diff word wrap | `diffWordWrap` | P1 | todo |
| Accessible diff viewer | `accessibleDiffViewerNext` | P3 | todo |
| Size / time limits | `maxComputationTime`, `maxFileSize` | P0 | done |
| Multi-file diff | `createMultiFileDiffEditor` | P2 | todo |
| Hunk / line patch building (gitgud) | not in Monaco, gitgud's `monaco-patch-builder.ts` | P0 | done |
| Three-way merge editor | not in standalone Monaco, gitgud's `merge-editor` | P1 | done |

## 12. Theming

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Built-in themes | `vs`, `vs-dark`, `hc-black`, `hc-light` | P0 | done |
| Custom themes inheriting a base | `defineTheme({base, inherit, rules, colors})` | P0 | done |
| Token color rules | `ITokenThemeRule {token, foreground, background, fontStyle}` | P0 | done |
| Workbench colors (~200 keys) | `colors: {'editor.background': …}` | P0 | done |
| Load TextMate / VS Code themes | not in Monaco | P2 | partial: `.tmTheme` and two-face's themes |

## 13. View state, layout, events

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Save / restore view state (scroll, cursors, folds) | `saveViewState`, `restoreViewState` | P0 | done |
| Reveal line / range (center, top, if outside) | `revealLine`, `revealLineInCenter`, `revealRange*`, `ScrollType` | P0 | done |
| Coordinates ↔ positions | `getScrolledVisiblePosition`, `getTargetAtClientPoint`, `getTopForLineNumber` | P1 | done (`EditorView::geometry`, `MouseTarget`) |
| Mouse targets (gutter, line number, glyph, content, scrollbar…) | `onMouseDown` + `MouseTargetType` | P0 | done |
| Content changed / cursor / scroll / focus events | `onDidChangeModelContent`, `onDidChangeCursorSelection`, `onDidScrollChange`, `onDidFocusEditorText` | P0 | done |
| Auto layout | `automaticLayout` | P0 | todo (free with an immediate-mode UI) |
| Accessibility (screen readers) | `accessibilitySupport`, `ariaLabel` | P2 | partial (named text input, value = cursor line; no full text or selection exposed) |
