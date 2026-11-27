//! Parsing on a thread of its own: the text, then the languages embedded in it.

use super::*;

/// The text of syntax nodes, read from the rope chunk by chunk (for `#match?` predicates).
pub(super) struct RopeText<'text>(pub(super) &'text Text);

impl<'text> TextProvider<&'text [u8]> for RopeText<'text> {
    type I = Box<dyn Iterator<Item = &'text [u8]> + 'text>;

    fn text(&mut self, node: Node) -> Self::I {
        Box::new(self.0.byte_chunks(node.start_byte(), node.end_byte()))
    }
}

/// The tree-sitter point (row, byte column) of byte `byte_offset`.
pub(super) fn point_of_byte(text: &Text, byte_offset: usize) -> Point {
    let line = text.line_of_offset(text.char_of_byte(byte_offset));
    Point::new(line, byte_offset - text.line_start_byte(line))
}

/// The point after `inserted`, inserted at `start`.
pub(super) fn point_after(start: Point, inserted: &str) -> Point {
    match inserted.rsplit_once('\n') {
        Some((before_last_line, last_line)) => {
            Point::new(start.row + before_last_line.matches('\n').count() + 1, last_line.len())
        }
        None => Point::new(start.row, start.column + inserted.len()),
    }
}

/// `change` on `text_before`, as tree-sitter takes it.
pub(super) fn input_edit(text_before: &Text, change: &Change) -> InputEdit {
    let start_byte = text_before.byte_of_char(change.start);
    let old_end_byte = text_before.byte_of_char(change.end);
    let start_position = point_of_byte(text_before, start_byte);
    InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte: start_byte + change.insert.len(),
        start_position,
        old_end_position: point_of_byte(text_before, old_end_byte),
        new_end_position: point_after(start_position, &change.insert),
    }
}

/// A text to parse, from the trees of an earlier version edited to match it.
pub(super) struct ParseJob {
    pub(super) generation: u64,
    pub(super) text: Text,
    /// The previous tree of each layer.
    pub(super) old_trees: Vec<Option<Tree>>,
}

pub(super) struct ParsedTrees {
    pub(super) generation: u64,
    /// The tree of each layer; `None` for an embedded language the text holds none of.
    pub(super) trees: Vec<Option<Tree>>,
    pub(super) parse_duration: Duration,
}

/// Parses the text, then each embedded language over the parts the injection query captures in
/// the text's tree: all the parts of one language in one tree, as if they were one file (the
/// scripts of a page share their names). `None` when parsing was cancelled.
pub(super) fn parse_layers(
    parsers: &mut [Parser],
    host: &Grammar,
    layer_of_pattern: &[Option<usize>],
    job: &ParseJob,
) -> Option<Vec<Option<Tree>>> {
    let read_chunk = &mut |byte_offset: usize, _: Point| job.text.chunk_from_byte(byte_offset);
    let host_tree = parsers[0].parse_with_options(read_chunk, job.old_trees[0].as_ref(), None)?;

    let mut layer_ranges: Vec<Vec<tree_sitter::Range>> = vec![Vec::new(); parsers.len()];
    if let Some(injections) = &host.injections {
        let content_capture = injections.capture_index_for_name("injection.content");
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(injections, host_tree.root_node(), RopeText(&job.text));
        while let Some(found) = matches.next() {
            let Some(layer) = layer_of_pattern[found.pattern_index] else { continue };
            let contents = found.captures().iter().filter(|capture| Some(capture.index) == content_capture);
            let ranges = contents.map(|capture| capture.node.range()).filter(|range| range.start_byte < range.end_byte);
            layer_ranges[layer].extend(ranges);
        }
    }

    let mut trees = vec![Some(host_tree)];
    for (layer, ranges) in layer_ranges.iter_mut().enumerate().skip(1) {
        ranges.sort_by_key(|range| range.start_byte);
        let parser = &mut parsers[layer];
        let has_parts = !ranges.is_empty() && parser.set_included_ranges(ranges).is_ok();
        let tree = has_parts.then(|| parser.parse_with_options(read_chunk, job.old_trees[layer].as_ref(), None));
        trees.push(tree.flatten());
    }
    Some(trees)
}

/// A thread parsing jobs one after the other; it ends when its highlighter is dropped.
pub(super) struct BackgroundParser {
    pub(super) jobs: Sender<ParseJob>,
    pub(super) parsed: Receiver<ParsedTrees>,
}

impl BackgroundParser {
    /// Parses with `grammars`: the text's own first, then those `layer_of_pattern` names.
    pub(super) fn spawn(grammars: Vec<Arc<Grammar>>, layer_of_pattern: Vec<Option<usize>>) -> Self {
        let (jobs, job_receiver) = channel::<ParseJob>();
        let (parsed_sender, parsed) = channel();
        std::thread::spawn(move || {
            let mut parsers: Vec<Parser> = grammars
                .iter()
                .map(|grammar| {
                    let mut parser = Parser::new();
                    parser.set_language(&grammar.language).expect("bundled grammars match the tree-sitter version");
                    parser
                })
                .collect();
            for job in job_receiver {
                let started = Instant::now();
                let Some(trees) = parse_layers(&mut parsers, &grammars[0], &layer_of_pattern, &job) else { continue };
                let parsed = ParsedTrees { generation: job.generation, trees, parse_duration: started.elapsed() };
                if parsed_sender.send(parsed).is_err() {
                    break;
                }
            }
        });
        Self { jobs, parsed }
    }
}

/// The grammars of the languages embedded in `grammar`'s text, and the layer each pattern of
/// its injection query parses (1 for the first embedded grammar: layer 0 is the text's own).
/// A language without a bundled grammar is left out.
pub(super) fn embedded_grammars(
    grammar: &Grammar,
    registry: &LanguageRegistry,
) -> (Vec<Arc<Grammar>>, Vec<Option<usize>>) {
    let Some(injections) = &grammar.injections else { return (Vec::new(), Vec::new()) };
    let mut names: Vec<&str> = Vec::new();
    let mut grammars = Vec::new();
    let layer_of_pattern = (0..injections.pattern_count())
        .map(|pattern| {
            let language = injections
                .property_settings(pattern)
                .iter()
                .find(|property| &*property.key == "injection.language")
                .and_then(|property| property.value.as_deref())
                .and_then(injected_language_name)?;
            if let Some(index) = names.iter().position(|name| *name == language) {
                return Some(index + 1);
            }
            grammars.push(registry.grammar(language)?);
            names.push(language);
            Some(grammars.len())
        })
        .collect();
    (grammars, layer_of_pattern)
}
