//! Search that forgives typos: "witcer", "hollw knight" and "gtav" find their games. Case,
//! accents and Turkish letters do not matter; each typed word has to match a word of the
//! title (as a part of it, as its start with a letter or two wrong, or as initials).

/// Lower case, without accents (`ş` → `s`, `é` → `e`, `ı` → `i`).
pub fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'ı' | 'í' | 'ì' | 'î' | 'ï' => 'i',
            'ş' | 'ś' => 's',
            'ğ' => 'g',
            'ü' | 'ú' | 'ù' | 'û' => 'u',
            'ö' | 'ó' | 'ò' | 'ô' | 'õ' => 'o',
            'ç' | 'ć' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

fn words(s: &str) -> Vec<String> {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Edit distance (insert, delete, change, swap of neighbours).
fn distance(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[n][m]
}

/// How many mistakes a typed word of `len` letters may have.
fn allowed(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4..=6 => 1,
        _ => 2,
    }
}

/// How well `word` (typed) matches the title's word `w`: 0 is exact or a part of it, more is
/// worse; `None` = it does not.
fn word_score(word: &str, w: &str) -> Option<usize> {
    if w.contains(word) {
        return Some(0);
    }
    let q: Vec<char> = word.chars().collect();
    let t: Vec<char> = w.chars().collect();
    let k = allowed(q.len());
    if k == 0 {
        return None;
    }
    // The whole word, or the start of the title's word as long as what was typed.
    let whole = distance(&q, &t);
    let start = distance(&q, &t[..t.len().min(q.len())]);
    let best = whole.min(start);
    (best <= k).then_some(best + 1)
}

/// How well `query` matches `text`: `None` when it does not, otherwise lower is better (0 =
/// every word found as it was typed). An empty query matches everything.
pub fn score(query: &str, text: &str) -> Option<usize> {
    let q = words(query);
    if q.is_empty() {
        return Some(0);
    }
    let folded = fold(text);
    let joined: String = folded.chars().filter(|c| c.is_alphanumeric()).collect();
    let t = words(text);
    // Initials ("gtav", "rdr2") or the title without spaces ("farcry").
    let one: String = q.concat();
    if q.len() == 1 && one.len() >= 2 {
        let initials: String = t.iter().filter_map(|w| w.chars().next()).collect();
        let initials_digits: String = t
            .iter()
            .map(|w| {
                if w.chars().all(|c| c.is_ascii_digit()) {
                    w.clone()
                } else {
                    w.chars().next().map(String::from).unwrap_or_default()
                }
            })
            .collect();
        if initials.starts_with(&one) || initials_digits.starts_with(&one) || joined.contains(&one)
        {
            return Some(0);
        }
    }
    let mut total = 0;
    for word in &q {
        let best = t.iter().filter_map(|w| word_score(word, w)).min()?;
        total += best;
    }
    Some(total)
}

pub fn matches(query: &str, text: &str) -> bool {
    score(query, text).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgives_typos_and_finds_initials() {
        assert!(matches("witcer", "The Witcher 3: Wild Hunt"));
        assert!(matches("hollw knight", "Hollow Knight"));
        assert!(matches("gtav", "Grand Theft Auto V"));
        assert!(matches("rdr2", "Red Dead Redemption 2"));
        assert!(matches("farcry", "Far Cry 5"));
        assert!(matches("SEHIR", "Şehir Oyunu"));
        assert!(matches("eldn ring", "ELDEN RING"));
        assert!(!matches("doom", "Hollow Knight"));
        assert!(!matches("cat", "Control"), "short words must be exact");
        assert_eq!(score("", "anything"), Some(0));
        assert!(score("hollow", "Hollow Knight") < score("holow", "Hollow Knight"));
    }
}
