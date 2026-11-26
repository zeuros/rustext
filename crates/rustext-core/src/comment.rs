//! Toggling line and block comments, from the language configuration's comment tokens.

use crate::change::{Change, ChangeSet};
use crate::edit::{EditContext, Replacement, Transaction, replace_each};
use crate::history::EditKind;
use crate::indentation::leading_whitespace_len;
use crate::lines::line_blocks;

/// Ctrl+/: comments the covered lines, or uncomments them when every non-empty one is
/// commented. The comment token goes at the smallest indentation of the block, followed by a
/// space. Languages without a line comment use their block comment around the lines.
pub fn toggle_line_comment(context: &EditContext) -> Option<Transaction> {
    let Some(token) = context.language.line_comment.as_deref() else {
        return toggle_block_comment(context);
    };
    let text = context.text;
    let token_len = token.chars().count();

    let changes = line_blocks(text, context.selections).into_iter().flat_map(|(first_line, last_line)| {
        let lines: Vec<(usize, String)> = (first_line..=last_line)
            .map(|line| (line, text.line_content(line)))
            .filter(|(_, content)| !content.trim().is_empty())
            .collect();
        let is_commented =
            !lines.is_empty() && lines.iter().all(|(_, content)| content.trim_start().starts_with(token));
        let min_indent = lines.iter().map(|(_, content)| leading_whitespace_len(content)).min().unwrap_or(0);

        lines
            .into_iter()
            .map(move |(line, content)| {
                let line_start = text.line_start(line);
                if !is_commented {
                    return Change::insert(line_start + min_indent, format!("{token} "));
                }
                let token_start = line_start + leading_whitespace_len(&content);
                let after_token: String =
                    content.chars().skip(leading_whitespace_len(&content) + token_len).take(1).collect();
                let removed_len = token_len + usize::from(after_token == " ");
                Change::delete(token_start, token_start + removed_len)
            })
            .collect::<Vec<_>>()
    });
    let changes = ChangeSet::new(changes);
    if changes.is_empty() {
        return None;
    }
    let selections = context.selections.map(&changes);
    Some(Transaction { changes, selections, kind: EditKind::Other, auto_closed_offsets: Vec::new() })
}

/// Shift+Alt+A: wraps each selection in the block comment tokens, or unwraps it when it is
/// already wrapped. An empty selection gets an empty comment with the cursor inside.
pub fn toggle_block_comment(context: &EditContext) -> Option<Transaction> {
    let (open, close) = context.language.block_comment.as_ref()?;
    let text = context.text;
    let (open_len, close_len) = (open.chars().count(), close.chars().count());

    replace_each(context, EditKind::Other, |selection| {
        let (start, end) = (selection.start(), selection.end());
        let selected = text.slice(start, end);
        let trimmed = selected.trim();
        if trimmed.len() >= open.len() + close.len()
            && trimmed.starts_with(open.as_str())
            && trimmed.ends_with(close.as_str())
        {
            let inner = &trimmed[open.len()..trimmed.len() - close.len()];
            let inner = inner.strip_prefix(' ').unwrap_or(inner);
            let inner = inner.strip_suffix(' ').unwrap_or(inner);
            let leading = &selected[..selected.len() - selected.trim_start().len()];
            let trailing = &selected[selected.trim_end().len()..];
            let unwrapped = format!("{leading}{inner}{trailing}");
            let unwrapped_len = unwrapped.chars().count();
            return Some(Replacement::new(start, end, unwrapped).with_selection(0, unwrapped_len));
        }
        if selection.is_empty() {
            return Some(Replacement::new(start, end, format!("{open}  {close}")).with_cursor(open_len + 1));
        }
        let wrapped = format!("{open} {selected} {close}");
        let selected_len = end - start;
        Some(Replacement::new(start, end, wrapped).with_selection(0, open_len + selected_len + close_len + 2))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EditingConfig, LanguageConfiguration};
    use crate::selection::{Selection, Selections};
    use crate::text::Text;

    fn run(content: &str, selections: Selections, language: LanguageConfiguration, block: bool) -> String {
        let text = Text::new(content);
        let config = EditingConfig::default();
        let context = EditContext {
            text: &text,
            selections: &selections,
            config: &config,
            language: &language,
            auto_closed_offsets: &[],
        };
        let transaction = if block { toggle_block_comment(&context) } else { toggle_line_comment(&context) };
        transaction.expect("a transaction").changes.apply(&text).to_string()
    }

    #[test]
    fn comments_at_the_smallest_indentation_and_skips_empty_lines() {
        let content = "  a\n\n    b";
        let commented = run(content, Selections::single(Selection::new(0, 10)), LanguageConfiguration::c_like(), false);

        assert_eq!(commented, "  // a\n\n  //   b");
    }

    #[test]
    fn uncomments_when_every_line_is_commented() {
        let commented = "  // a\n  //   b";
        let uncommented =
            run(commented, Selections::single(Selection::new(0, 15)), LanguageConfiguration::c_like(), false);

        assert_eq!(uncommented, "  a\n    b");
    }

    #[test]
    fn mixed_lines_get_commented() {
        let content = "// a\nb";
        let commented = run(content, Selections::single(Selection::new(0, 6)), LanguageConfiguration::c_like(), false);

        assert_eq!(commented, "// // a\n// b");
    }

    #[test]
    fn block_comment_wraps_and_unwraps() {
        let language = LanguageConfiguration::c_like();
        let wrapped = run("x = 1", Selections::single(Selection::new(0, 5)), language.clone(), true);
        assert_eq!(wrapped, "/* x = 1 */");

        let unwrapped = run(&wrapped, Selections::single(Selection::new(0, 11)), language, true);
        assert_eq!(unwrapped, "x = 1");
    }

    #[test]
    fn languages_without_line_comments_use_block_comments() {
        let html = LanguageConfiguration::plain_text().with_block_comment("<!--", "-->");

        assert_eq!(run("a", Selections::single(Selection::new(0, 1)), html, false), "<!-- a -->");
    }
}
