//! What is read from a syntax tree: spans, bracket columns, symbols, folds and nodes.

use super::*;

/// The byte ranges a tree was parsed over: the whole text, or the parts of an embedded language.
pub(super) fn parsed_byte_ranges(tree: &Tree) -> Vec<std::ops::Range<usize>> {
    tree.included_ranges().iter().map(|range| range.start_byte..range.end_byte).collect()
}

/// Adds the columns of the bracket chars of a `text` node to the lines of `columns_of_lines`.
pub(super) fn push_prose_bracket_columns(
    node: Node,
    text: &Text,
    lines: &std::ops::Range<usize>,
    columns_of_lines: &mut [Vec<usize>],
) {
    let node_chars = text.char_of_byte(node.start_byte())..text.char_of_byte(node.end_byte());
    for line in node.start_position().row.max(lines.start)..=node.end_position().row.min(lines.end - 1) {
        let line_start = text.line_start(line);
        let content = text.line_content(line);
        let bracket_columns = content.chars().enumerate().filter(|&(column, character)| {
            node_chars.contains(&(line_start + column)) && BRACKET_CHARS.contains(character)
        });
        columns_of_lines[line - lines.start].extend(bracket_columns.map(|(column, _)| column));
    }
}

/// The columns of the bracket chars of the tokens `query` captures on each of `lines`, in the
/// ranges `tree` was parsed over.
pub(super) fn bracket_columns_of_lines(
    query: &Query,
    cursor: &mut QueryCursor,
    tree: &Tree,
    text: &Text,
    lines: std::ops::Range<usize>,
) -> Vec<Vec<usize>> {
    let last_line = lines.end - 1;
    let block_end = text.line_start_byte(last_line) + text.line_content(last_line).len();
    cursor.set_byte_range(text.line_start_byte(lines.start)..block_end);
    let parsed_ranges = parsed_byte_ranges(tree);
    let prose_capture = query.capture_index_for_name("text");
    let mut columns_of_lines = vec![Vec::new(); lines.len()];
    let mut matches = cursor.matches(query, tree.root_node(), RopeText(text));
    while let Some(found) = matches.next() {
        for capture in found.captures() {
            let token = capture.node;
            if Some(capture.index) == prose_capture {
                push_prose_bracket_columns(token, text, &lines, &mut columns_of_lines);
                continue;
            }
            let line = token.start_position().row;
            let is_parsed = parsed_ranges.iter().any(|range| range.contains(&token.start_byte()));
            // An alias may name a token after other text: its kind must be its text.
            if token.is_missing()
                || !is_parsed
                || !lines.contains(&line)
                || token.byte_range().len() != token.kind().len()
            {
                continue;
            }
            let first_column = text.char_of_byte(token.start_byte()) - text.line_start(line);
            let bracket_columns = token
                .kind()
                .chars()
                .enumerate()
                .filter(|(_, character)| BRACKET_CHARS.contains(*character))
                .map(|(index, _)| first_column + index);
            columns_of_lines[line - lines.start].extend(bracket_columns);
        }
    }
    for columns in &mut columns_of_lines {
        columns.sort_unstable();
        columns.dedup();
    }
    columns_of_lines
}

/// The symbol kind of a tags query's `definition.<kind>` capture.
pub(super) fn symbol_kind(tag_kind: &str) -> SymbolKind {
    match tag_kind {
        "function" | "macro" => SymbolKind::Function,
        "method" => SymbolKind::Method,
        "class" | "interface" | "type" | "struct" | "enum" | "union" | "trait" => SymbolKind::Type,
        "module" | "namespace" => SymbolKind::Module,
        "constant" => SymbolKind::Constant,
        "field" | "property" => SymbolKind::Field,
        "variable" => SymbolKind::Variable,
        _ => SymbolKind::Other,
    }
}

/// Words that end a block on a line of their own, kept visible by its fold.
pub(super) const CLOSING_KEYWORDS: &[&str] = &["end", "fi", "done", "esac", "endif", "endfor", "endwhile"];

pub(super) fn is_closing_bracket(node: Node) -> bool {
    matches!(node.kind(), "}" | "]" | ")")
}

/// Whether a multi-line node folds from the line it starts on: a bracketed node (`{…}`,
/// `[…]`, `(…)`) does; a node ending with a bracket doesn't (`if a {…} else {…}` folds
/// through its blocks, not as a whole); another node (a Python `def`, a Ruby `def…end`, a
/// comment, an HTML element) does unless it is the tail of a construct started on an
/// earlier line (a Python `block`, a YAML value).
pub(super) fn folds_from_its_first_line(node: Node) -> bool {
    let last_index = node.child_count().saturating_sub(1);
    let is_bracketed = node.child_count() >= 2
        && node.child(0).is_some_and(|child| matches!(child.kind(), "{" | "[" | "("))
        && node.child(last_index).is_some_and(is_closing_bracket);
    if is_bracketed {
        return true;
    }
    let mut last_leaf = node;
    while let Some(child) = last_leaf.child(last_leaf.child_count().saturating_sub(1)) {
        last_leaf = child;
    }
    let is_tail = !node.kind().contains("comment")
        && node.parent().is_some_and(|parent| {
            let is_last_named_child =
                parent.named_child(parent.named_child_count().saturating_sub(1) as u32) == Some(node);
            parent.start_position().row < node.start_position().row && is_last_named_child
        });
    !is_closing_bracket(last_leaf) && !is_tail
}

/// A line being highlighted: where it is, and the captures crossing it as
/// `(layer, node length, pattern index, start column, end column, style)`.
pub(super) struct LineCaptures {
    start_byte: usize,
    char_starts: Vec<usize>,
    captured: Vec<(usize, usize, usize, usize, usize, Option<TextStyle>)>,
}

impl LineCaptures {
    fn end_byte(&self, text: &Text, line: usize) -> usize {
        self.start_byte + text.line_content(line).len()
    }

    fn column_of(&self, byte_offset: usize) -> usize {
        self.char_starts.partition_point(|&start| start < byte_offset)
    }

    /// The spans: an embedded language wins over the text's own, a smaller node over the node
    /// holding it, and of two patterns capturing the same node the first.
    fn into_spans(mut self) -> Vec<StyledSpan> {
        self.captured
            .sort_by_key(|&(layer, node_len, pattern_index, ..)| (layer, Reverse(node_len), Reverse(pattern_index)));
        let mut column_styles: Vec<Option<TextStyle>> = vec![None; self.char_starts.len()];
        for (_, _, _, start_column, end_column, style) in self.captured {
            column_styles[start_column..end_column].fill(style);
        }
        let mut spans: Vec<StyledSpan> = Vec::new();
        for (column, style) in column_styles.into_iter().enumerate() {
            let Some(style) = style else { continue };
            match spans.last_mut() {
                Some(last) if last.end_column == column && last.style == style => last.end_column = column + 1,
                _ => spans.push(StyledSpan { start_column: column, end_column: column + 1, style }),
            }
        }
        spans
    }
}

/// Adds a capture of `bytes` to the lines of `block` it crosses, ranked by
/// `(layer, node length, pattern index)`.
fn capture_on_lines(
    block: &mut [LineCaptures],
    bytes: std::ops::Range<usize>,
    rank: (usize, usize, usize),
    style: Option<TextStyle>,
) {
    let (layer, node_len, pattern_index) = rank;
    let first = block.partition_point(|line| line.start_byte <= bytes.start).saturating_sub(1);
    for line in block[first..].iter_mut().take_while(|line| line.start_byte < bytes.end) {
        let (start_column, end_column) = (line.column_of(bytes.start), line.column_of(bytes.end));
        if start_column < end_column {
            line.captured.push((layer, node_len, pattern_index, start_column, end_column, style));
        }
    }
}

/// The spans of `lines` from the captures of every layer's highlight query crossing them, one
/// query per layer; a layer's captures are cut to the parts it parsed. Lines longer than
/// `max_line_chars` get none.
pub(super) fn highlight_lines(
    layers: &[Layer],
    cursor: &mut QueryCursor,
    text: &Text,
    lines: std::ops::Range<usize>,
    max_line_chars: usize,
) -> Vec<Vec<StyledSpan>> {
    let mut block: Vec<LineCaptures> = lines
        .clone()
        .map(|line| {
            let start_byte = text.line_start_byte(line);
            let content = text.line_content(line);
            let char_starts = if content.chars().count() > max_line_chars {
                Vec::new()
            } else {
                content.char_indices().map(|(byte, _)| start_byte + byte).collect()
            };
            LineCaptures { start_byte, char_starts, captured: Vec::new() }
        })
        .collect();
    let block_start = block[0].start_byte;
    let block_end = block[block.len() - 1].end_byte(text, lines.end - 1);

    for (layer_index, layer) in layers.iter().enumerate() {
        let Some(tree) = &layer.tree else { continue };
        let parsed_ranges = parsed_byte_ranges(tree);
        cursor.set_byte_range(block_start..block_end);
        let mut captures = cursor.captures(&layer.grammar.highlights, tree.root_node(), RopeText(text));
        while let Some((query_match, capture_index)) = captures.next() {
            let capture = query_match.captures()[*capture_index];
            let node_bytes = capture.node.byte_range();
            let style = layer.capture_styles[capture.index as usize];
            let rank = (layer_index, node_bytes.len(), query_match.pattern_index);
            for parsed in &parsed_ranges {
                let bytes = node_bytes.start.max(parsed.start)..node_bytes.end.min(parsed.end);
                capture_on_lines(&mut block, bytes, rank, style);
            }
        }
    }
    block.into_iter().map(LineCaptures::into_spans).collect()
}

/// The byte range of the smallest node of `tree` holding `start_byte..end_byte` and larger
/// than it.
pub(super) fn enclosing_node(tree: &Tree, start_byte: usize, end_byte: usize) -> Option<std::ops::Range<usize>> {
    let mut node = tree.root_node().descendant_for_byte_range(start_byte, end_byte)?;
    while node.start_byte() > start_byte
        || node.end_byte() < end_byte
        || (node.start_byte() == start_byte && node.end_byte() == end_byte)
    {
        node = node.parent()?;
    }
    Some(node.byte_range())
}

/// The `@definition.*` matches of `tags` in `tree`, named by their `@name`.
pub(super) fn tagged_symbols(tags: &Query, tree: &Tree, text: &Text) -> Vec<DocumentSymbol> {
    let capture_names = tags.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(tags, tree.root_node(), RopeText(text));
    let mut symbols = Vec::new();
    while let Some(found) = matches.next() {
        let captures = found.captures();
        let kind =
            captures.iter().find_map(|capture| capture_names[capture.index as usize].strip_prefix("definition."));
        let name = captures.iter().find(|capture| capture_names[capture.index as usize] == "name");
        let (Some(kind), Some(name)) = (kind, name) else { continue };
        let (start, end) = (text.char_of_byte(name.node.start_byte()), text.char_of_byte(name.node.end_byte()));
        symbols.push(DocumentSymbol {
            name: text.slice(start, end),
            kind: symbol_kind(kind),
            range: Range::new(text.position(start), text.position(end)),
            container: None,
        });
    }
    symbols
}

/// Looks at the nodes of `tree` starting on `line`, token by token (so `} else {` starts the
/// `else`); of those that fold (`folds_from_its_first_line`), the one ending last gives the
/// range, its last line kept visible when it only closes (`}`, `end`, `</div>`).
pub(super) fn syntax_fold_range_in(tree: &Tree, text: &Text, line: usize) -> Option<FoldRange> {
    let content = text.line_content(line);
    let mut byte_column = content.len() - content.trim_start().len();
    let mut fold_end = Point::new(line, 0);
    while byte_column < content.len() {
        let point = Point::new(line, byte_column);
        let token = tree.root_node().descendant_for_point_range(point, point)?;
        let mut node = token;
        while let Some(parent) = node.parent().filter(|_| node.start_position().row == line) {
            if node.end_position().row > line && folds_from_its_first_line(node) {
                fold_end = fold_end.max(node.end_position());
            }
            node = parent;
        }
        let token_end = token.end_position();
        byte_column = if token_end.row == line && token_end.column > byte_column {
            token_end.column
        } else {
            content[byte_column..].char_indices().nth(1).map_or(content.len(), |(next, _)| byte_column + next)
        };
    }
    let last_line_content = text.line_content(fold_end.row.min(text.last_line()));
    let before_end = last_line_content.get(..fold_end.column).unwrap_or(&last_line_content);
    let closing = before_end.trim();
    let only_closes = closing.chars().all(|character| ")]};,".contains(character))
        || CLOSING_KEYWORDS.contains(&closing)
        || (closing.starts_with("</") && closing.ends_with('>') && !closing[2..].contains('<'));
    let last_hidden_line = if only_closes { fold_end.row.saturating_sub(1) } else { fold_end.row };
    (last_hidden_line > line).then_some(FoldRange { header_line: line, last_hidden_line })
}
