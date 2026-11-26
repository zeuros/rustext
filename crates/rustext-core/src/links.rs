//! Links in the text (Monaco's link detection): `https://`, `http://`, `file://` and
//! `mailto:` URLs, without the punctuation that ends a sentence around them.

use std::sync::OnceLock;

use regex::Regex;

/// A link in a line, in char columns.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Link {
    pub start_column: usize,
    pub end_column: usize,
    pub url: String,
}

fn url_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(r#"(?:https?://|file://|mailto:)[^\s<>"'`]+"#).expect("the link pattern compiles"))
}

/// `url` without trailing punctuation (`.`, `,`, `;`…) or a closing bracket it doesn't open
/// (`(see https://a.b/c)` → `https://a.b/c`; `https://a.b/f(x)` keeps its `)`).
fn trimmed(url: &str) -> &str {
    let mut url = url;
    loop {
        let Some(last) = url.chars().last() else { return url };
        let opening = match last {
            ')' => Some('('),
            ']' => Some('['),
            '}' => Some('{'),
            _ => None,
        };
        let is_unbalanced_closer = opening.is_some_and(|opening| {
            url.chars().filter(|&character| character == opening).count()
                < url.chars().filter(|&character| character == last).count()
        });
        if !".,;:!?".contains(last) && !is_unbalanced_closer {
            return url;
        }
        url = &url[..url.len() - last.len_utf8()];
    }
}

/// The links of a line.
pub fn links_in_line(line_content: &str) -> Vec<Link> {
    url_pattern()
        .find_iter(line_content)
        .filter_map(|found| {
            let url = trimmed(found.as_str());
            let is_bare_scheme = url.ends_with("://") || url.ends_with(':');
            (!is_bare_scheme).then(|| {
                let start_column = line_content[..found.start()].chars().count();
                Link { start_column, end_column: start_column + url.chars().count(), url: url.to_string() }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(line: &str) -> Vec<String> {
        links_in_line(line).into_iter().map(|link| link.url).collect()
    }

    #[test]
    fn finds_urls_with_their_schemes() {
        assert_eq!(
            urls("see https://a.dev/x?q=1 or mailto:me@a.dev and file:///tmp/a.txt"),
            vec!["https://a.dev/x?q=1", "mailto:me@a.dev", "file:///tmp/a.txt"]
        );
    }

    #[test]
    fn sentence_punctuation_and_unopened_brackets_are_not_part_of_a_url() {
        assert_eq!(urls("(see https://a.dev/docs)."), vec!["https://a.dev/docs"]);
        assert_eq!(urls("https://en.wiki/F(x), done"), vec!["https://en.wiki/F(x)"]);
        assert_eq!(urls("\"https://a.dev\""), vec!["https://a.dev"]);
    }

    #[test]
    fn columns_are_chars() {
        let links = links_in_line("é → https://a.dev");

        assert_eq!((links[0].start_column, links[0].end_column), (4, 17));
    }

    #[test]
    fn a_scheme_alone_is_no_link() {
        assert!(urls("the https:// prefix").is_empty());
    }
}
