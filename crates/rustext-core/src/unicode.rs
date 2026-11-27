//! Suspicious characters (Monaco's `unicodeHighlight`, DECISIONS D32): invisible ones (zero
//! width spaces, bidi controls) and ones that look like an ASCII character (a Cyrillic `а` in
//! `pаypal`, a Greek question mark for `;`). Code read by people can do what they don't see.

/// Why a character is pointed out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SuspiciousKind {
    /// It shows nothing, or only space.
    Invisible,
    /// It looks like this ASCII character.
    Ambiguous { looks_like: char },
    /// Any character outside basic ASCII (Monaco's `nonBasicASCII`, off by default).
    NonBasicAscii,
}

/// A suspicious character of a line, at a char column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SuspiciousChar {
    pub column: usize,
    pub character: char,
    pub kind: SuspiciousKind,
}

impl SuspiciousChar {
    /// What a hover says about it: `U+0430 'а' looks like ASCII 'a'`.
    pub fn explanation(&self) -> String {
        let code = format!("U+{:04X}", u32::from(self.character));
        match self.kind {
            SuspiciousKind::Invisible => format!("{code} is invisible"),
            SuspiciousKind::Ambiguous { looks_like } => {
                format!("{code} '{}' looks like ASCII '{looks_like}'", self.character)
            }
            SuspiciousKind::NonBasicAscii => format!("{code} '{}' is not basic ASCII", self.character),
        }
    }
}

/// Which characters are pointed out (Monaco's `unicodeHighlight.*`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UnicodeHighlight {
    pub invisible_characters: bool,
    pub ambiguous_characters: bool,
    pub non_basic_ascii: bool,
    /// Characters never pointed out (`allowedCharacters`).
    pub allowed_characters: Vec<char>,
}

impl Default for UnicodeHighlight {
    fn default() -> Self {
        Self {
            invisible_characters: true,
            ambiguous_characters: true,
            non_basic_ascii: false,
            allowed_characters: Vec::new(),
        }
    }
}

/// Whether `character` shows nothing, or a space unlike the ASCII one: format controls, bidi
/// controls (Trojan Source), zero width and odd-width spaces, fillers.
pub fn is_invisible(character: char) -> bool {
    matches!(
        character,
        '\u{00A0}'
            | '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{17B4}'
            | '\u{17B5}'
            | '\u{180E}'
            | '\u{2000}'..='\u{200F}'
            | '\u{2028}'..='\u{202F}'
            | '\u{205F}'..='\u{206F}'
            | '\u{3000}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// Letters of other scripts drawn like an ASCII letter or digit, and what they look like.
const LOOKALIKE_LETTERS: &[(char, char)] = &[
    // Cyrillic.
    ('а', 'a'),
    ('в', 'B'),
    ('е', 'e'),
    ('һ', 'h'),
    ('і', 'i'),
    ('ј', 'j'),
    ('к', 'k'),
    ('о', 'o'),
    ('р', 'p'),
    ('с', 'c'),
    ('ѕ', 's'),
    ('у', 'y'),
    ('х', 'x'),
    ('ԁ', 'd'),
    ('ԛ', 'q'),
    ('ԝ', 'w'),
    ('А', 'A'),
    ('В', 'B'),
    ('Е', 'E'),
    ('З', '3'),
    ('І', 'I'),
    ('Ј', 'J'),
    ('К', 'K'),
    ('М', 'M'),
    ('Н', 'H'),
    ('О', 'O'),
    ('Р', 'P'),
    ('С', 'C'),
    ('Ѕ', 'S'),
    ('Т', 'T'),
    ('Х', 'X'),
    ('Ү', 'Y'),
    // Greek.
    ('ο', 'o'),
    ('ν', 'v'),
    ('Α', 'A'),
    ('Β', 'B'),
    ('Ε', 'E'),
    ('Ζ', 'Z'),
    ('Η', 'H'),
    ('Ι', 'I'),
    ('Κ', 'K'),
    ('Μ', 'M'),
    ('Ν', 'N'),
    ('Ο', 'O'),
    ('Ρ', 'P'),
    ('Τ', 'T'),
    ('Υ', 'Y'),
    ('Χ', 'X'),
    // Latin extensions.
    ('ı', 'i'),
    ('ǀ', 'l'),
    ('ɡ', 'g'),
];

/// Punctuation drawn like an ASCII one; pointed out wherever it is.
const LOOKALIKE_PUNCTUATION: &[(char, char)] = &[
    ('\u{037E}', ';'),
    ('\u{0589}', ':'),
    ('\u{2010}', '-'),
    ('\u{2011}', '-'),
    ('\u{2212}', '-'),
    ('\u{2044}', '/'),
    ('\u{2215}', '/'),
    ('\u{01C3}', '!'),
    ('\u{02BC}', '\''),
    ('\u{2024}', '.'),
    ('\u{A789}', ':'),
];

/// The ASCII character `character` looks like, if any: the tables above, and the fullwidth
/// forms of ASCII (`ａ`, `；`).
fn looks_like(character: char) -> Option<char> {
    if ('\u{FF01}'..='\u{FF5E}').contains(&character) {
        return char::from_u32(u32::from(character) - 0xFEE0);
    }
    LOOKALIKE_LETTERS
        .iter()
        .chain(LOOKALIKE_PUNCTUATION)
        .find(|(lookalike, _)| *lookalike == character)
        .map(|&(_, ascii)| ascii)
}

/// Whether the word around `column` mixes look-alikes only with ASCII: `pаypal` and `ехес` do,
/// Russian `привет` (with letters drawn like no ASCII one) doesn't.
fn is_in_a_disguised_word(chars: &[char], column: usize) -> bool {
    let is_word_char = |character: &char| character.is_alphanumeric() || *character == '_';
    let start = chars[..column].iter().rposition(|character| !is_word_char(character)).map_or(0, |index| index + 1);
    let end = chars[column..]
        .iter()
        .position(|character| !is_word_char(character))
        .map_or(chars.len(), |index| column + index);
    chars[start..end].iter().all(|&character| character.is_ascii() || looks_like(character).is_some())
}

/// The suspicious characters of `line_content`, by column.
pub fn suspicious_chars(line_content: &str, options: &UnicodeHighlight) -> Vec<SuspiciousChar> {
    let chars: Vec<char> = line_content.chars().collect();
    let mut found = Vec::new();
    for (column, &character) in chars.iter().enumerate() {
        if character.is_ascii() || options.allowed_characters.contains(&character) {
            continue;
        }
        let kind = if is_invisible(character) {
            options.invisible_characters.then_some(SuspiciousKind::Invisible)
        } else {
            let ambiguous = looks_like(character)
                .filter(|_| options.ambiguous_characters)
                .filter(|_| !character.is_alphanumeric() || is_in_a_disguised_word(&chars, column));
            match ambiguous {
                Some(looks_like) => Some(SuspiciousKind::Ambiguous { looks_like }),
                None => options.non_basic_ascii.then_some(SuspiciousKind::NonBasicAscii),
            }
        };
        if let Some(kind) = kind {
            found.push(SuspiciousChar { column, character, kind });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(usize, SuspiciousKind)> {
        suspicious_chars(line, &UnicodeHighlight::default()).iter().map(|found| (found.column, found.kind)).collect()
    }

    #[test]
    fn a_cyrillic_letter_in_an_ascii_word_looks_like_ascii() {
        assert_eq!(kinds("let pаypal = 1;"), vec![(5, SuspiciousKind::Ambiguous { looks_like: 'a' })]);
    }

    #[test]
    fn a_word_of_look_alikes_only_is_disguised_too() {
        assert_eq!(kinds("ехес").len(), 4);
    }

    #[test]
    fn words_of_another_language_are_not_pointed_out() {
        assert_eq!(kinds("// привет, мир"), vec![]);
    }

    #[test]
    fn a_greek_question_mark_ends_a_statement_unseen() {
        assert_eq!(kinds("x = 1\u{037E}"), vec![(5, SuspiciousKind::Ambiguous { looks_like: ';' })]);
    }

    #[test]
    fn zero_width_and_bidi_characters_are_invisible() {
        assert_eq!(kinds("a\u{200B}b\u{202E}c"), vec![(1, SuspiciousKind::Invisible), (3, SuspiciousKind::Invisible)]);
    }

    #[test]
    fn fullwidth_forms_look_like_their_ascii_character() {
        assert_eq!(kinds("ａ"), vec![(0, SuspiciousKind::Ambiguous { looks_like: 'a' })]);
    }

    #[test]
    fn allowed_characters_and_switched_off_kinds_are_not_pointed_out() {
        let options = UnicodeHighlight {
            ambiguous_characters: false,
            allowed_characters: vec!['\u{00A0}'],
            ..UnicodeHighlight::default()
        };

        assert_eq!(suspicious_chars("pаypal\u{00A0}\u{200B}", &options).len(), 1);
    }

    #[test]
    fn non_basic_ascii_points_out_every_other_character() {
        let options = UnicodeHighlight { non_basic_ascii: true, ..UnicodeHighlight::default() };

        assert_eq!(suspicious_chars("é ü", &options).len(), 2);
    }

    #[test]
    fn the_explanation_names_the_code_point_and_its_look_alike() {
        let found = suspicious_chars("pаypal", &UnicodeHighlight::default())[0];

        assert_eq!(found.explanation(), "U+0430 'а' looks like ASCII 'a'");
    }
}
