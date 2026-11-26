# Monaco feature inventory

Source: `monaco-editor` 0.58 typings (`monaco.d.ts`, read from `../gitgud/node_modules`), the
built-in action IDs in `esm/vs/editor/contrib`, and the official playground's sample list.

Each feature lists the Monaco API it comes from, then its priority here:

- **P0**: gitgud uses it today (see "What gitgud uses"), needed to replace Monaco there.
- **P1**: what makes Monaco feel like Monaco while editing; done right after P0.
- **P2**: language-service surface (providers), done once the core is solid.
- **P3**: nice to have, or web-specific; maybe never.

`Status` is updated as milestones land (see [PLAN.md](PLAN.md)): `todo`, `wip`, `done`, `skip`.

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
| Text storage, line access | `getValue`, `getLineContent`, `getLineCount`, `getLinesContent`, `getValueInRange` | P0 | todo |
| Offset ↔ position | `getOffsetAt`, `getPositionAt`, `validatePosition`, `validateRange`, `modifyPosition` | P0 | todo |
| End of line handling | `getEOL`, `setEOL`, `pushEOL`, `EndOfLineSequence`, `DefaultEndOfLine` | P0 | todo |
| Edits | `applyEdits`, `pushEditOperations` (with cursor computer) | P0 | todo |
| Undo / redo with stops | `undo`, `redo`, `canUndo`, `pushStackElement`, `popStackElement` | P1 | todo |
| Version ids | `getVersionId`, `getAlternativeVersionId` (dirty tracking across undo) | P1 | todo |
| Find | `findMatches`, `findNextMatch`, `findPreviousMatch` (regex, case, whole word, in range) | P1 | todo |
| Words | `getWordAtPosition`, `getWordUntilPosition`, `wordSeparators` | P1 | todo |
| Indentation | `detectIndentation`, `normalizeIndentation`, `tabSize`, `insertSpaces`, `indentSize` | P1 | todo |
| Whitespace columns | `getLineFirstNonWhitespaceColumn`, `getLineLastNonWhitespaceColumn` | P1 | todo |
| Snapshots | `createSnapshot` (stream the text) | P3 | todo |
| Model registry by URI | `createModel(value, lang, uri)`, `getModel(uri)`, `getModels` | — | skip: see DECISIONS D6 |

## 2. Cursors and selections

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Single cursor, selection | `setPosition`, `setSelection`, `getSelection`, `onDidChangeCursorPosition` | P0 | todo |
| Multi-cursor | `setSelections`, `getSelections`, `multiCursorModifier`, `multiCursorMergeOverlapping`, `multiCursorLimit` | P1 | todo |
| Add cursor above / below | `editor.action.insertCursorAbove/Below` (Ctrl+Alt+↑/↓) | P1 | todo |
| Add next occurrence | `editor.action.addSelectionToNextFindMatch` (Ctrl+D), `moveSelectionToNextFindMatch` (Ctrl+K Ctrl+D), `selectHighlights` (Ctrl+Shift+L) | P1 | todo |
| Cursors at line ends | `insertCursorAtEndOfEachLineSelected` (Shift+Alt+I) | P1 | todo |
| Column (box) selection | `columnSelection`, Shift+Alt+drag, `cursorColumnSelect*` | P1 | todo |
| Cursor motions | `cursorLeft/Right/Up/Down/Home/End/Top/Bottom/PageUp/PageDown` (+`Select`) | P0 | todo |
| Word motions | `cursorWordLeft/Right`, `cursorWordStart*/End*`, `cursorWordPart*` (camelCase parts) | P1 | todo |
| Smart Home (first non-whitespace, then column 1) | `cursorHome` | P1 | todo |
| Sticky column on vertical moves | implicit | P0 | todo |
| Cursor undo | `cursorUndo` / `cursorRedo` (Ctrl+U) | P2 | todo |
| Selection anchor | `setSelectionAnchor`, `selectFromAnchorToCursor` | P3 | todo |
| Smart select expand / shrink | `editor.action.smartSelect.expand/shrink` (Shift+Alt+→/←) | P2 | todo |
| Cursor style, blinking, width, smooth caret | `cursorStyle`, `cursorBlinking`, `cursorWidth`, `cursorSmoothCaretAnimation` | P1 | todo |
| Cursor surrounding lines | `cursorSurroundingLines` | P2 | todo |

## 3. Editing behaviour

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Typing, backspace, delete, enter | type / deleteLeft / deleteRight | P0 | todo |
| Auto-indent on Enter | `autoIndent`, `onEnterRules`, `indentationRules` | P1 | todo |
| Auto-closing pairs | `autoClosingBrackets`, `autoClosingQuotes`, `autoClosingDelete`, `autoClosingOvertype`, language `autoClosingPairs` | P1 | todo |
| Surround selection | `autoSurround`, language `surroundingPairs` | P1 | todo |
| Tab / Shift+Tab indent / outdent | `editor.action.indentLines`, `outdentLines`, `useTabStops`, `tabFocusMode` | P1 | todo |
| Line comment / block comment | `editor.action.commentLine` (Ctrl+/), `blockComment` (Shift+Alt+A), language `comments` | P1 | todo |
| Move lines up / down | `moveLinesUpAction` (Alt+↑/↓) | P1 | todo |
| Copy lines up / down | `copyLinesUpAction` (Shift+Alt+↑/↓) | P1 | todo |
| Delete line | `deleteLines` (Ctrl+Shift+K) | P1 | todo |
| Duplicate selection | `duplicateSelection` | P1 | todo |
| Join lines | `joinLines` | P1 | todo |
| Sort lines, reverse, remove duplicates | `sortLinesAscending/Descending`, `reverseLines`, `removeDuplicateLines` | P1 | todo |
| Transpose letters | `transposeLetters` | P2 | todo |
| Case transforms | `transformToUppercase/Lowercase/Titlecase/Snakecase/Camelcase/Kebabcase/Pascalcase` | P1 | todo |
| Trim trailing whitespace | `editor.action.trimTrailingWhitespace` | P1 | todo |
| Insert final newline | contrib `insertFinalNewLine` | P2 | todo |
| Clipboard | `clipboardCopyAction`, `Cut`, `Paste`, `emptySelectionClipboard` (copy whole line), `multiCursorPaste` (spread), `copyWithSyntaxHighlighting` | P0 | todo |
| Overtype mode (Insert key) | `overtypeCursorStyle`, `overtypeOnPaste` | P2 | todo |
| In-place replace (cycle true/false…) | `inPlaceReplace.up/down` | P3 | todo |
| Read-only + message | `readOnly`, `readOnlyMessage`, `onDidAttemptReadOnlyEdit` | P0 | todo |
| IME composition | `onDidCompositionStart/End`, `inComposition` | P0 | todo |
| Linked editing (HTML tag pairs) | `linkedEditing`, `registerLinkedEditingRangeProvider` | P2 | todo |
| Snippets with tab stops | `snippetSuggestions`, `insertTextRules.InsertAsSnippet` | P2 | todo |
| Drag and drop text | `dragAndDrop`, `dropIntoEditor` | P3 | todo |

## 4. Rendering

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Line numbers: on, off, relative, interval, custom | `lineNumbers` (`RenderLineNumbersType`), `lineNumbersMinChars` | P0 | todo |
| Current line highlight | `renderLineHighlight` (`line`, `gutter`, `all`), `renderLineHighlightOnlyWhenFocus` | P0 | todo |
| Selection painting, rounded | `roundedSelection` | P0 | todo |
| Whitespace rendering | `renderWhitespace` (`none`, `boundary`, `selection`, `trailing`, `all`) | P1 | todo |
| Control characters | `renderControlCharacters` (and visible CR, the gitgud hack done right) | P0 | todo |
| Rulers | `rulers` (columns, colors) | P1 | todo |
| Indent guides, active guide | `guides.indentation`, `guides.highlightActiveIndentation` | P1 | todo |
| Bracket pair colorization | `bracketPairColorization.enabled`, `independentColorPoolPerBracketType` | P1 | todo |
| Bracket pair guides | `guides.bracketPairs`, `bracketPairsHorizontal` | P2 | todo |
| Matching bracket highlight, jump | `matchBrackets`, `editor.action.jumpToBracket` (Ctrl+Shift+\), `selectToBracket` | P1 | todo |
| Selection occurrences highlight | `selectionHighlight` | P1 | todo |
| Word occurrences highlight | `occurrencesHighlight` (`singleFile`, `multiFile`), `wordHighlight.next/prev` | P1 | todo |
| Word wrap | `wordWrap` (`off`, `on`, `wordWrapColumn`, `bounded`), `wrappingIndent`, `wordWrapBreakBeforeCharacters` | P1 | todo |
| Scrolling | `scrollBeyondLastLine`, `scrollBeyondLastColumn`, `smoothScrolling`, `mouseWheelScrollSensitivity`, `fastScrollSensitivity` (Alt), `scrollPredominantAxis` | P0 | todo |
| Scrollbars with overview ruler | `scrollbar.*`, `overviewRulerLanes`, `overviewRulerBorder`, `hideCursorInOverviewRuler` | P0 | todo |
| Minimap | `minimap.enabled/side/size/scale/renderCharacters/showSlider/autohide`, section headers (`// MARK:`) | P2 | todo |
| Sticky scroll (scopes pinned at the top) | `stickyScroll.enabled/maxLineCount/defaultModel` | P2 | todo |
| Padding top / bottom | `padding` | P1 | todo |
| Fonts, ligatures, line height, letter spacing, zoom | `fontFamily`, `fontSize`, `fontLigatures`, `lineHeight`, `letterSpacing`, `mouseWheelZoom`, `fontZoomIn/Out/Reset` | P0 | todo |
| Unicode highlighting (invisible, ambiguous chars) | `unicodeHighlight.*` | P2 | todo |
| Unusual line terminators (LS, PS, NEL) | `unusualLineTerminators` | P2 | todo |
| Long lines: stop rendering after N chars | `stopRenderingLineAfter` | P1 | todo |
| Placeholder text when empty | `placeholder` | P2 | todo |
| Final newline rendering | `renderFinalNewline` | P2 | todo |
| Variable line heights / fonts | `allowVariableLineHeights`, decoration `lineHeight` | P3 | todo |

## 5. Decorations and widgets

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Range / inline decorations | `createDecorationsCollection`, `deltaDecorations`, `IModelDecorationOptions.className/inlineClassName` | P0 | todo |
| Whole-line decorations | `isWholeLine`, `marginClassName`, `lineNumberClassName` | P0 | todo |
| Glyph margin icons | `glyphMargin`, `glyphMarginClassName`, `GlyphMarginLane`, `addGlyphMarginWidget` | P1 | todo |
| Line decorations (left of the text, e.g. git gutter bars) | `linesDecorationsClassName`, `lineDecorationsWidth` | P1 | todo |
| Overview ruler marks | `overviewRuler: {color, position}` | P0 | todo |
| Minimap marks | `minimap: {color, position}` | P2 | todo |
| Injected text (before / after content) | `before`, `after` (`InjectedTextOptions`) | P2 | todo |
| Decoration hover message | `hoverMessage`, `glyphMarginHoverMessage` | P2 | todo |
| Stickiness when editing at the edges | `TrackedRangeStickiness` | P0 | todo |
| View zones (space between lines, e.g. diff padding, inline widgets) | `changeViewZones`, `IViewZone` | P0 (diff alignment) | todo |
| Content widgets (anchored at a position) | `addContentWidget` | P2 | todo |
| Overlay widgets (fixed in the editor, e.g. find) | `addOverlayWidget` | P1 | todo |
| Markers (diagnostics, squiggles) | `setModelMarkers`, `IMarkerData` (severity, tags `Unnecessary`, `Deprecated`), `showUnused`, `showDeprecated` | P1 | todo |
| Go to next problem | `editor.action.marker.next` (F8) | P2 | todo |
| Banner | `setBanner` | P3 | todo |

## 6. Find and replace (`contrib/find`)

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Find widget (Ctrl+F), next / previous (Enter, F3) | `actions.find`, `find.*` options | P0 | todo |
| Case, whole word, regex toggles | Alt+C, Alt+W, Alt+R | P1 | todo |
| Replace, replace all, regex groups `$1` | `editor.action.startFindReplaceAction` (Ctrl+H) | P1 | todo |
| Preserve case in replace | Alt+P | P2 | todo |
| Find in selection | `find.autoFindInSelection` | P1 | todo |
| Seed from selection / word | `find.seedSearchStringFromSelection` | P1 | todo |
| Match count "3 of 17", highlights in overview ruler | — | P1 | todo |
| Select all matches as cursors | Alt+Enter | P1 | todo |

## 7. Folding (`contrib/folding`)

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Indentation-based folding | `foldingStrategy: 'indentation'` | P1 | todo |
| Language / provider folding | `registerFoldingRangeProvider`, `foldingStrategy: 'auto'` | P2 | todo |
| Marker regions (`#region` / `// region`) | language `folding.markers`, `foldAllMarkerRegions` | P2 | todo |
| Fold / unfold / toggle, recursive, all, level N | `editor.fold`, `unfold`, `toggleFold`, `foldAll`, `unfoldAll`, `foldLevel1..7`, `foldRecursively` | P1 | todo |
| Fold controls in the gutter | `showFoldingControls` (`always`, `mouseover`, `never`) | P1 | todo |
| Folded region highlight, `…` after the line | `foldingHighlight` | P1 | todo |
| Unfold on click after end of line | `unfoldOnClickAfterEndOfLine` | P2 | todo |
| Hidden areas (generic, used by diff hunk view) | `onDidChangeHiddenAreas`, `setHiddenAreas` | P0 | todo |

## 8. Syntax and languages (`languages.*`)

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Syntax highlighting of many languages | Monarch (`setMonarchTokensProvider`), ~80 basic languages | P0 | todo |
| Language configuration | `setLanguageConfiguration` (comments, brackets, autoClosingPairs, surroundingPairs, folding, wordPattern, indentationRules, onEnterRules) | P1 | todo |
| Language registry and detection | `register`, `getLanguages`, `onLanguage` (by extension, filename, first line) | P0 | todo |
| Custom tokenizer | `setTokensProvider` (`IState`, line-by-line) | P2 | todo |
| Semantic tokens | `registerDocumentSemanticTokensProvider` | P3 | todo |
| Standalone colorize (to HTML) | `colorize`, `colorizeElement`, `colorizeModelLine`, `tokenize` | P2 | todo |
| Inspect tokens (debug) | `editor.action.inspectTokens` | P2 | todo |

## 9. Language services (providers)

All `languages.register*Provider`. Design here: see DECISIONS D9.

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Completion (suggest widget, fuzzy filter, details, commit chars, snippets) | `registerCompletionItemProvider`, `quickSuggestions`, `suggest.*`, `acceptSuggestionOnEnter`, `tabCompletion`, `wordBasedSuggestions` | P2 | todo |
| Hover | `registerHoverProvider`, `hover.delay/sticky/above` | P2 | todo |
| Signature help | `registerSignatureHelpProvider`, `parameterHints` | P2 | todo |
| Go to definition / declaration / type / implementation, Ctrl+click | `register*DefinitionProvider`, `gotoLocation.*`, `definitionLinkOpensInPeek` | P2 | todo |
| References, peek view | `registerReferenceProvider`, `peekView` | P2 | todo |
| Document highlights | `registerDocumentHighlightProvider` | P2 | todo |
| Rename (F2) | `registerRenameProvider`, `registerNewSymbolNameProvider` | P2 | todo |
| Code actions, lightbulb, quick fix | `registerCodeActionProvider`, `lightbulb.enabled` | P2 | todo |
| Code lens | `registerCodeLensProvider`, `codeLens` | P2 | todo |
| Formatting (document, range, on type, on paste) | `registerDocumentFormattingEditProvider`, `…Range…`, `…OnType…`, `formatOnType`, `formatOnPaste` | P2 | todo |
| Inlay hints | `registerInlayHintsProvider`, `inlayHints.*` | P2 | todo |
| Inline completions (ghost text, AI) | `registerInlineCompletionsProvider`, `inlineSuggest.*` | P2 | todo |
| Links (Ctrl+click URLs) | `registerLinkProvider`, `links`, `registerLinkOpener` | P1 | todo |
| Color decorators and picker | `registerColorProvider`, `colorDecorators` | P3 | todo |
| Document symbols, outline, Go to symbol (Ctrl+Shift+O) | `registerDocumentSymbolProvider`, quick access `@` | P2 | todo |
| Selection ranges (smart select) | `registerSelectionRangeProvider` | P2 | todo |
| LSP bridge | not in Monaco (monaco-languageclient) | P2 | todo |

## 10. Commands, keybindings, menus

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Built-in commands (~150 ids listed above) | `editor.getAction(id).run()`, `trigger(source, id)` | P0 | todo |
| Custom actions with keybinding + context menu entry | `addAction({id, label, keybindings, contextMenuGroupId, precondition, run})` | P0 | todo |
| Keybinding rules, chords (Ctrl+K Ctrl+C) | `addKeybindingRule(s)`, `KeyMod`, `KeyCode` | P1 | todo |
| Context keys (`when` clauses) | `createContextKey`, `precondition` | — | skip: see DECISIONS D8 |
| Context menu | `contextmenu` | P0 | todo |
| Command palette (F1) | quick access `>` | P2 | todo |
| Go to line (Ctrl+G) | quick access `:` | P1 | todo |

## 11. Diff editor (`IDiffEditor`) — gitgud's main use

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Line diff + char/word-level inner changes | `getLineChanges()` (`ILineChange.charChanges`), `diffAlgorithm` (`legacy`, `advanced`) | P0 | todo |
| Side-by-side, aligned with padding zones | `renderSideBySide: true` | P0 | todo |
| Inline (unified) view | `renderSideBySide: false` | P0 | todo |
| Auto inline when narrow | `useInlineViewWhenSpaceIsLimited`, `renderSideBySideInlineBreakpoint` | P1 | todo |
| Collapse unchanged regions, expand on click | `hideUnchangedRegions.{enabled, contextLineCount, minimumLineCount, revealLineCount}` | P0 | todo |
| Ignore trim whitespace | `ignoreTrimWhitespace` | P0 | todo |
| +/- indicators, revert arrows in the gutter | `renderIndicators`, `renderMarginRevertIcon`, `renderGutterMenu` | P1 | todo |
| Overview ruler of changes | `renderOverviewRuler` | P0 | todo |
| Resizable split | `enableSplitViewResizing`, `splitViewDefaultRatio` | P1 | todo |
| Navigate changes (F7 / Shift+F7) | `goToDiff('next'|'previous')`, `revealFirstDiff` | P0 | todo |
| Original side editable | `originalEditable` | P2 | todo |
| Moved code detection | `experimental.showMoves` | P2 | todo |
| Diff word wrap | `diffWordWrap` | P1 | todo |
| Accessible diff viewer | `accessibleDiffViewerNext` | P3 | todo |
| Size / time limits | `maxComputationTime`, `maxFileSize` | P0 | todo |
| Multi-file diff | `createMultiFileDiffEditor` | P2 | todo |
| Hunk / line patch building (gitgud) | not in Monaco, gitgud's `monaco-patch-builder.ts` | P0 | todo |
| Three-way merge editor | not in standalone Monaco, gitgud's `merge-editor` | P1 | todo |

## 12. Theming

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Built-in themes | `vs`, `vs-dark`, `hc-black`, `hc-light` | P0 | todo |
| Custom themes inheriting a base | `defineTheme({base, inherit, rules, colors})` | P0 | todo |
| Token color rules | `ITokenThemeRule {token, foreground, background, fontStyle}` | P0 | todo |
| Workbench colors (~200 keys) | `colors: {'editor.background': …}` | P0 | todo |
| Load TextMate / VS Code themes | not in Monaco | P2 | todo |

## 13. View state, layout, events

| Feature | Monaco API | Pri | Status |
|---|---|---|---|
| Save / restore view state (scroll, cursors, folds) | `saveViewState`, `restoreViewState` | P0 | todo |
| Reveal line / range (center, top, if outside) | `revealLine`, `revealLineInCenter`, `revealRange*`, `ScrollType` | P0 | todo |
| Coordinates ↔ positions | `getScrolledVisiblePosition`, `getTargetAtClientPoint`, `getTopForLineNumber` | P1 | todo |
| Mouse targets (gutter, line number, glyph, content, scrollbar…) | `onMouseDown` + `MouseTargetType` | P0 | todo |
| Content changed / cursor / scroll / focus events | `onDidChangeModelContent`, `onDidChangeCursorSelection`, `onDidScrollChange`, `onDidFocusEditorText` | P0 | todo |
| Auto layout | `automaticLayout` | P0 | todo (free with an immediate-mode UI) |
| Accessibility (screen readers) | `accessibilitySupport`, `ariaLabel` | P2 | todo |
