//! Linked editing of tag pairs (Monaco's `linkedEditing`, DECISIONS D34): renaming `<div>`
//! renames its `</div>`. Tags are read from the text, in HTML, XML, JSX and the like.

use crate::text::Text;

/// A tag name, as char offsets: `start..end`.
pub type NameRange = (usize, usize);

/// Elements HTML never closes: `<br>` is not waiting for a `</br>`.
const VOID_ELEMENTS: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];

/// Elements whose content is not markup: a `<` in a script is a comparison.
const RAW_TEXT_ELEMENTS: &[&str] = &["script", "style"];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Tag {
    name: NameRange,
    is_close: bool,
    /// `<br/>`, or a void element: it has no pair.
    is_self_closing: bool,
}

/// Whether `character` may be in a tag name (`my-element`, `svg:rect`, `Foo.Bar`).
pub fn is_tag_name_char(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '-' | ':' | '_' | '.')
}

/// The name of the tag holding `offset` (its start and end included) and the name of its pair,
/// in that order. `None` outside a tag name, or for a tag without a pair.
pub fn tag_pair_names(text: &Text, offset: usize) -> Option<[NameRange; 2]> {
    let chars: Vec<char> = text.slice(0, text.len_chars()).chars().collect();
    let tags = tags_of(&chars);
    let index = tags.iter().position(|tag| (tag.name.0..=tag.name.1).contains(&offset) && !tag.is_self_closing)?;
    let name_of = |tag: &Tag| &chars[tag.name.0..tag.name.1];
    let is_same_name = |tag: &&Tag| !tag.is_self_closing && name_of(tag) == name_of(&tags[index]);

    let mut depth = 0usize;
    let pair = if tags[index].is_close {
        tags[..index].iter().rev().filter(is_same_name).find(|tag| is_pair(tag, true, &mut depth))
    } else {
        tags[index + 1..].iter().filter(is_same_name).find(|tag| is_pair(tag, false, &mut depth))
    }?;
    Some([tags[index].name, pair.name])
}

/// Whether `tag` closes (or opens) the one searched from, nested pairs of the same name skipped.
fn is_pair(tag: &Tag, searches_open: bool, depth: &mut usize) -> bool {
    if tag.is_close == searches_open {
        *depth += 1;
        return false;
    }
    if *depth == 0 {
        return true;
    }
    *depth -= 1;
    false
}

/// The tags of the text in order, comments and raw text skipped.
fn tags_of(chars: &[char]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '<' {
            index += 1;
            continue;
        }
        if chars[index..].starts_with(&['<', '!', '-', '-']) {
            index = find(chars, index + 4, &['-', '-', '>']).map_or(chars.len(), |end| end + 3);
            continue;
        }
        let Some((tag, tag_end)) = tag_at(chars, index) else {
            index += 1;
            continue;
        };
        index = tag_end;
        let name: String = chars[tag.name.0..tag.name.1].iter().collect();
        if !tag.is_close && !tag.is_self_closing && RAW_TEXT_ELEMENTS.contains(&name.to_ascii_lowercase().as_str()) {
            let closing: Vec<char> = format!("</{name}").chars().collect();
            index = find(chars, index, &closing).unwrap_or(chars.len());
        }
        tags.push(tag);
    }
    tags
}

/// The tag starting at the `<` at `start`, and the offset after its `>` (or where it stops).
fn tag_at(chars: &[char], start: usize) -> Option<(Tag, usize)> {
    let is_close = chars.get(start + 1) == Some(&'/');
    let name_start = start + 1 + usize::from(is_close);
    // An empty name (`<>`, a JSX fragment, or a name being retyped) still pairs.
    if !chars.get(name_start).is_some_and(|&character| character.is_alphabetic() || character == '>') {
        return None;
    }
    let name_end =
        name_start + chars[name_start..].iter().take_while(|&&character| is_tag_name_char(character)).count();

    let mut quote = None;
    let mut index = name_end;
    while index < chars.len() {
        match (chars[index], quote) {
            ('"' | '\'', None) => quote = Some(chars[index]),
            (character, Some(open)) if character == open => quote = None,
            ('>', None) => break,
            ('<', None) => return None,
            _ => {}
        }
        index += 1;
    }
    let name: String = chars[name_start..name_end].iter().collect();
    let ends_with_slash = index > name_end && chars[index - 1] == '/';
    let is_void = VOID_ELEMENTS.contains(&name.to_ascii_lowercase().as_str());
    let tag =
        Tag { name: (name_start, name_end), is_close, is_self_closing: !is_close && (ends_with_slash || is_void) };
    Some((tag, (index + 1).min(chars.len())))
}

/// The first offset from `from` where `needle` starts.
fn find(chars: &[char], from: usize, needle: &[char]) -> Option<usize> {
    chars.get(from..)?.windows(needle.len()).position(|window| window == needle).map(|index| from + index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair_names(content: &str, offset: usize) -> Option<[String; 2]> {
        let text = Text::new(content);
        tag_pair_names(&text, offset).map(|names| names.map(|(start, end)| text.slice(start, end)))
    }

    #[test]
    fn an_open_tag_name_pairs_with_its_close_tag() {
        let names = tag_pair_names(&Text::new("<div class=\"a\">x</div>"), 2);

        assert_eq!(names, Some([(1, 4), (18, 21)]));
    }

    #[test]
    fn a_close_tag_name_pairs_with_its_open_tag() {
        assert_eq!(tag_pair_names(&Text::new("<p>x</p>"), 7), Some([(6, 7), (1, 2)]));
    }

    #[test]
    fn nested_tags_of_the_same_name_pair_by_depth() {
        let content = "<div><div></div></div>";

        assert_eq!(tag_pair_names(&Text::new(content), 1), Some([(1, 4), (18, 21)]));
        assert_eq!(tag_pair_names(&Text::new(content), 6), Some([(6, 9), (12, 15)]));
    }

    #[test]
    fn void_and_self_closing_tags_have_no_pair() {
        assert_eq!(pair_names("<br><img/></br>", 1), None);
        assert_eq!(pair_names("<x/></x>", 1), None);
    }

    #[test]
    fn comments_and_scripts_are_not_markup() {
        let content = "<b><!-- </b> --><script>if (a<b) {}</script></b>";

        assert_eq!(pair_names(content, 1), Some(["b".into(), "b".into()]));
        assert_eq!(tag_pair_names(&Text::new(content), 1).map(|names| names[1].0), Some(content.len() - 2));
    }

    #[test]
    fn a_greater_than_in_an_attribute_value_does_not_end_the_tag() {
        assert!(pair_names("<a title=\"x > y\">z</a>", 1).is_some());
    }

    #[test]
    fn empty_names_pair_while_a_name_is_retyped() {
        assert_eq!(tag_pair_names(&Text::new("<>x</>"), 1), Some([(1, 1), (5, 5)]));
    }

    #[test]
    fn outside_a_tag_name_there_is_no_pair() {
        assert_eq!(pair_names("<div class=\"a\">x</div>", 6), None);
        assert_eq!(pair_names("let less = a < b;", 13), None);
    }
}
