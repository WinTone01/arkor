//! Folding titles for comparison (from Irori): Latin and Greek accents dropped, trademark
//! signs left out, other scripts kept.

/// The plain letters of an accented Latin or Greek lower-case letter;
/// `None` for any other character.
pub fn unaccent(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' | 'ț' => "t",
        'þ' => "th",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        // Greek: tonos and dialytika, and the final sigma.
        'ά' => "α",
        'έ' => "ε",
        'ή' => "η",
        'ί' | 'ϊ' | 'ΐ' => "ι",
        'ό' => "ο",
        'ύ' | 'ϋ' | 'ΰ' => "υ",
        'ώ' => "ω",
        'ς' => "σ",
        _ => return None,
    })
}

/// Marks that say nothing about which game a title names, dropped without
/// splitting the word around them: ™, ®, ©, emoji variation selectors and
/// zero-width joiners.
pub fn is_ignorable(c: char) -> bool {
    matches!(
        c,
        '™' | '®' | '©' | '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}'
    )
}

/// `c` in lower case with Latin and Greek accents dropped, ignorable marks
/// left out, each output character passed to `push`.
pub fn fold_char(c: char, mut push: impl FnMut(char)) {
    if is_ignorable(c) {
        return;
    }
    for lower in c.to_lowercase() {
        match unaccent(lower) {
            Some(plain) => plain.chars().for_each(&mut push),
            None => push(lower),
        }
    }
}
