//! Title normalization: two titles that normalize to the same text are
//! treated as the same title.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::{decompose_compatible, is_combining_mark};
use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

/// Normalizes a title for lookup in the title index. The steps and their
/// order follow Taiga, so that titles match the same way they do there.
pub fn normalize(title: &str) -> String {
    let mut s = title.to_owned();
    roman_numerals(&mut s);
    transliterate(&mut s);
    // Lower case from here on, because of case folding.
    s = normalize_unicode(&s);
    ordinals(&mut s);
    season_numbers(&mut s);
    for (before, after) in [
        ("&", "and"),
        ("the animation", ""),
        ("the", ""),
        ("episode", ""),
        ("oad", "ova"),
        ("oav", "ova"),
        ("specials", "sp"),
        ("special", "sp"),
        ("(tv)", ""),
    ] {
        replace_whole_word(&mut s, before, after);
    }
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.retain(|c| !is_removable(c));
    s
}

fn roman_numerals(s: &mut String) {
    // I and X are skipped because they are rarely numerals in titles, and
    // anything above XIII is rare enough not to need a real conversion.
    for (before, after) in [
        ("II", "2"),
        ("III", "3"),
        ("IV", "4"),
        ("V", "5"),
        ("VI", "6"),
        ("VII", "7"),
        ("VIII", "8"),
        ("IX", "9"),
        ("XI", "11"),
        ("XII", "12"),
        ("XIII", "13"),
    ] {
        replace_whole_word(s, before, after);
    }
}

fn transliterate(s: &mut String) {
    // Equivalences that Unicode lumping does not cover, and romanizations
    // that are always the same sound.
    *s = s
        .chars()
        .map(|c| match c {
            '@' => "a".into(),
            '×' => "x".into(),
            '\u{A789}' => ":".into(),
            'Ō' | 'ō' => "ou".into(),
            'ū' => "uu".into(),
            c => c.to_string(),
        })
        .collect();
    // Hepburn writes these particles as they sound, Wapuro as they are
    // spelled. Case sensitive, as in Taiga.
    replace_whole_word(s, "wa", "ha");
    replace_whole_word(s, "e", "he");
    replace_whole_word(s, "o", "wo");
}

fn ordinals(s: &mut String) {
    for (before, after) in [
        ("first", "1st"),
        ("second", "2nd"),
        ("third", "3rd"),
        ("fourth", "4th"),
        ("fifth", "5th"),
        ("sixth", "6th"),
        ("seventh", "7th"),
        ("eighth", "8th"),
        ("ninth", "9th"),
    ] {
        replace_whole_word(s, before, after);
    }
}

fn season_numbers(s: &mut String) {
    const ORDINALS: [&str; 6] = ["1st", "2nd", "3rd", "4th", "5th", "6th"];
    for (i, ordinal) in ORDINALS.iter().enumerate() {
        let n = (i + 1).to_string();
        for before in [
            format!("{ordinal} season"),
            format!("season {n}"),
            format!("series {n}"),
            format!("s{n}"),
        ] {
            replace_whole_word(s, &before, &n);
        }
    }
}

/// Characters deleted in the last step: anything up to U+00FF that is not a
/// letter or digit (spaces included), and the symbol block of stars, hearts
/// and notes.
fn is_removable(c: char) -> bool {
    let code = c as u32;
    (code <= 0xFF && !c.is_alphanumeric()) || (0x2001..=0x2766).contains(&code)
}

fn is_word_boundary(c: char) -> bool {
    c.is_whitespace() || c.general_category_group() == GeneralCategoryGroup::Punctuation
}

/// Replaces `before` with `after` where it is not directly next to another
/// word character. Like Taiga, only spaces and punctuation separate words, so
/// a symbol next to the match keeps it from matching.
fn replace_whole_word(s: &mut String, before: &str, after: &str) {
    if before.is_empty() || !s.contains(before) {
        return;
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    let mut previous: Option<char> = None;
    while let Some(pos) = rest.find(before) {
        let prefix = &rest[..pos];
        let char_before = prefix.chars().next_back().or(previous);
        let tail = &rest[pos + before.len()..];
        let starts_word = char_before.is_none_or(is_word_boundary);
        let ends_word = tail.chars().next().is_none_or(is_word_boundary);
        out.push_str(prefix);
        if starts_word && ends_word {
            out.push_str(after);
            previous = after.chars().next_back().or(char_before);
        } else {
            out.push_str(before);
            previous = before.chars().next_back();
        }
        rest = tail;
    }
    out.push_str(rest);
    *s = out;
}

/// NFKC, then strip default-ignorable characters, control characters and
/// combining marks, map look-alike punctuation to one form, and case fold.
///
/// This reproduces `utf8proc_map` with `COMPAT | COMPOSE | STABLE | IGNORE |
/// STRIPCC | STRIPMARK | LUMP | CASEFOLD`, which Taiga uses, step by step:
/// each character is filtered, lumped, case folded and decomposed in that
/// order, the pieces go through the same steps again, and the result is
/// composed at the end.
fn normalize_unicode(s: &str) -> String {
    let mut decomposed = Vec::with_capacity(s.len());
    for c in s.chars() {
        decompose_char(c, &mut decomposed);
    }
    let mut cleaned = String::with_capacity(decomposed.len());
    let mut chars = decomposed.iter().copied().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' | '\u{85}' | '\u{0B}' | '\u{0C}' | '\t' => cleaned.push(' '),
            c if (c as u32) < 0x20 || (0x7F..0xA0).contains(&(c as u32)) => {}
            c => cleaned.push(c),
        }
    }
    cleaned.nfc().collect()
}

fn decompose_char(c: char, out: &mut Vec<char>) {
    if is_default_ignorable(c) {
        return;
    }
    if let Some(lumped) = lump(c) {
        out.push(lumped);
        return;
    }
    if is_combining_mark(c) {
        return;
    }
    let folded: Vec<char> = caseless::Caseless::default_case_fold(std::iter::once(c)).collect();
    if folded != [c] {
        for f in folded {
            decompose_char(f, out);
        }
        return;
    }
    let mut parts = Vec::new();
    decompose_compatible(c, |d| parts.push(d));
    if parts.len() == 1 && parts[0] == c {
        out.push(c);
    } else {
        for part in parts {
            decompose_char(part, out);
        }
    }
}

/// utf8proc's `LUMP` table: look-alike characters mapped to one ASCII form.
fn lump(c: char) -> Option<char> {
    let category = c.general_category();
    let lumped = match c {
        _ if category == GeneralCategory::SpaceSeparator => ' ',
        '\u{2018}' | '\u{2019}' | '\u{02BC}' | '\u{02C8}' => '\'',
        _ if category == GeneralCategory::DashPunctuation => '-',
        '\u{2212}' => '-',
        '\u{2044}' | '\u{2215}' => '/',
        '\u{2236}' => ':',
        '\u{2039}' | '\u{2329}' | '\u{3008}' => '<',
        '\u{203A}' | '\u{232A}' | '\u{3009}' => '>',
        '\u{2216}' => '\\',
        '\u{02C4}' | '\u{02C6}' | '\u{2038}' | '\u{2303}' => '^',
        _ if category == GeneralCategory::ConnectorPunctuation => '_',
        '\u{02CD}' => '_',
        '\u{02CB}' => '`',
        '\u{2223}' => '|',
        '\u{223C}' => '~',
        _ => return None,
    };
    Some(lumped)
}

/// The Default_Ignorable_Code_Point property from Unicode's
/// DerivedCoreProperties.txt.
fn is_default_ignorable(c: char) -> bool {
    matches!(c as u32,
        0x00AD
        | 0x034F
        | 0x061C
        | 0x115F..=0x1160
        | 0x17B4..=0x17B5
        | 0x180B..=0x180F
        | 0x200B..=0x200F
        | 0x202A..=0x202E
        | 0x2060..=0x206F
        | 0x3164
        | 0xFE00..=0xFE0F
        | 0xFEFF
        | 0xFFA0
        | 0xFFF0..=0xFFF8
        | 0x1BCA0..=0x1BCA3
        | 0x1D173..=0x1D17A
        | 0xE0000..=0xE0FFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_examples() {
        for (input, output) in [
            ("Toradora!", "toradora"),
            ("The iDOLM@STER", "idolmaster"),
            ("Tasogare Otome × Amnesia", "tasogareotomexamnesia"),
            ("Attack on Titan Season 2", "attackontitan2"),
            ("Attack on Titan 2nd Season", "attackontitan2"),
            ("Fate/stay night (TV)", "fatestaynight"),
            (
                "Mahouka Koukou no Rettousei II",
                "mahoukakoukounorettousei2",
            ),
        ] {
            assert_eq!(normalize(input), output, "{input}");
        }
    }

    #[test]
    fn unicode_steps() {
        // Accents stripped, full-width letters folded, dashes lumped.
        assert_eq!(normalize("Pokémon"), "pokemon");
        assert_eq!(normalize("ＡＢＣ"), "abc");
        assert_eq!(normalize("Straße"), "strasse");
        assert_eq!(normalize("A\u{2013}B"), "ab");
        assert_eq!(normalize("Zero\u{200B}Width"), "zerowidth");
        assert_eq!(normalize("Kōkaku"), "koukaku");
        assert_eq!(normalize("Nisekoi\u{A789}"), "nisekoi");
        assert_eq!(normalize("Love Live! ☆ Sunshine♥"), "lovelivesunshine");
        // Japanese is kept; only marks and folding apply.
        assert_eq!(normalize("進撃の巨人"), "進撃の巨人");
        // Voicing marks decompose into combining marks and are stripped too,
        // exactly as utf8proc does. Both spellings still meet in the index.
        assert_eq!(normalize("ガンダム"), "カンタム");
    }

    #[test]
    fn particles_and_words() {
        assert_eq!(normalize("Kimi wa Kanata"), "kimihakanata");
        assert_eq!(
            normalize("Sen to Chihiro no Kamikakushi"),
            "sentochihironokamikakushi"
        );
        assert_eq!(normalize("Kanojo o Karimasu"), "kanojowokarimasu");
        assert_eq!(normalize("Hyouka & Co"), "hyoukaandco");
        assert_eq!(normalize("Bleach Specials"), "bleachsp");
        assert_eq!(normalize("Hellsing OAV"), "hellsingova");
        assert_eq!(normalize("Kanon Episode"), "kanon");
        assert_eq!(normalize("Clannad Second Season"), "clannad2");
        assert_eq!(normalize("Gintama S3"), "gintama3");
        assert_eq!(normalize("Gintama S7"), "gintamas7");
        assert_eq!(normalize("Rozen Maiden Series 2"), "rozenmaiden2");
    }

    #[test]
    fn whole_words_only() {
        assert_eq!(normalize("Theatre"), "theatre");
        assert_eq!(normalize("Vampire"), "vampire");
        assert_eq!(normalize("Code Geass R2 IV"), "codegeassr24");
        // A symbol is not a word boundary, so the numeral stays.
        assert_eq!(normalize("A×II"), "axii");
    }

    #[test]
    fn empty_and_punctuation_only() {
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("!!!"), "");
        assert_eq!(normalize("The"), "");
    }
}
