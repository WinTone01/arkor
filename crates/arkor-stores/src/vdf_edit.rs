//! Changes single values in a text `.vdf` file and leaves every other byte as it was
//! (order, comments, indentation, keys Arkor does not know). Used for Steam's own settings
//! files, which Steam keeps writing itself. Keys match case-insensitively, as Steam's do.

use anyhow::Context;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Str,
    Open,
    Close,
}

#[derive(Debug)]
struct Tok {
    kind: Kind,
    /// Byte range in the input, quotes included.
    start: usize,
    end: usize,
    text: String,
}

fn tokens(input: &str) -> anyhow::Result<Vec<Tok>> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' | b'}' => {
                let kind = if c == b'{' { Kind::Open } else { Kind::Close };
                out.push(Tok {
                    kind,
                    start: i,
                    end: i + 1,
                    text: String::new(),
                });
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                let mut text = Vec::new();
                loop {
                    let Some(&b) = bytes.get(i) else {
                        anyhow::bail!("unterminated string in vdf");
                    };
                    i += 1;
                    match b {
                        b'"' => break,
                        b'\\' => {
                            let Some(&n) = bytes.get(i) else {
                                anyhow::bail!("unterminated escape in vdf");
                            };
                            i += 1;
                            match n {
                                b'n' => text.push(b'\n'),
                                b't' => text.push(b'\t'),
                                other => text.push(other),
                            }
                        }
                        other => text.push(other),
                    }
                }
                let text = String::from_utf8(text).context("vdf string is not UTF-8")?;
                out.push(Tok {
                    kind: Kind::Str,
                    start,
                    end: i,
                    text,
                });
            }
            _ => {
                let start = i;
                while i < bytes.len() && !b" \t\r\n{}\"".contains(&bytes[i]) {
                    i += 1;
                }
                out.push(Tok {
                    kind: Kind::Str,
                    start,
                    end: i,
                    text: input[start..i].to_string(),
                });
            }
        }
    }
    Ok(out)
}

/// Index just past the value that starts at `i` (a string, or a block to its `}`).
fn skip_value(toks: &[Tok], i: usize) -> anyhow::Result<usize> {
    match toks.get(i).map(|t| t.kind) {
        Some(Kind::Str) => Ok(i + 1),
        Some(Kind::Open) => {
            let mut depth = 0;
            for (j, t) in toks.iter().enumerate().skip(i) {
                match t.kind {
                    Kind::Open => depth += 1,
                    Kind::Close => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(j + 1);
                        }
                    }
                    Kind::Str => {}
                }
            }
            anyhow::bail!("unbalanced braces in vdf")
        }
        _ => anyhow::bail!("expected a value in vdf"),
    }
}

enum Found {
    /// Token index of the key; its value starts right after it.
    Key(usize),
    /// `depth` parts of the path exist; new keys go before token `close` (the `}` of the
    /// deepest existing block), or at the end of the file when `None`.
    Missing { depth: usize, close: Option<usize> },
}

fn find(toks: &[Tok], path: &[&str]) -> anyhow::Result<Found> {
    let mut i = 0;
    let mut end = toks.len();
    let mut close = None;
    for (depth, part) in path.iter().enumerate() {
        let mut hit = None;
        while i < end {
            anyhow::ensure!(toks[i].kind == Kind::Str, "unexpected brace in vdf");
            let next = skip_value(toks, i + 1)?;
            if toks[i].text.eq_ignore_ascii_case(part) {
                hit = Some(i);
                break;
            }
            i = next;
        }
        let Some(key) = hit else {
            return Ok(Found::Missing { depth, close });
        };
        if depth + 1 == path.len() {
            return Ok(Found::Key(key));
        }
        anyhow::ensure!(
            toks.get(key + 1).map(|t| t.kind) == Some(Kind::Open),
            "`{part}` is not a block in vdf"
        );
        end = skip_value(toks, key + 1)? - 1;
        close = Some(end);
        i = key + 2;
    }
    anyhow::bail!("empty vdf path")
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Start of the line `pos` is on.
fn line_start(input: &str, pos: usize) -> usize {
    input[..pos].rfind('\n').map_or(0, |n| n + 1)
}

/// The string value at `path`, if there is one.
pub fn get(input: &str, path: &[&str]) -> anyhow::Result<Option<String>> {
    let toks = tokens(input)?;
    Ok(match find(&toks, path)? {
        Found::Key(k) => toks
            .get(k + 1)
            .filter(|t| t.kind == Kind::Str)
            .map(|t| t.text.clone()),
        Found::Missing { .. } => None,
    })
}

/// Keys of the block at `path` (empty path: the top level), in file order, as written.
pub fn keys(input: &str, path: &[&str]) -> anyhow::Result<Vec<String>> {
    let toks = tokens(input)?;
    let (mut i, end) = if path.is_empty() {
        (0, toks.len())
    } else {
        match find(&toks, path)? {
            Found::Key(k) if toks.get(k + 1).map(|t| t.kind) == Some(Kind::Open) => {
                (k + 2, skip_value(&toks, k + 1)? - 1)
            }
            _ => return Ok(Vec::new()),
        }
    };
    let mut out = Vec::new();
    while i < end {
        out.push(toks[i].text.clone());
        i = skip_value(&toks, i + 1)?;
    }
    Ok(out)
}

/// Sets the string at `path` (creating the blocks on the way); the rest stays as it was.
pub fn set(input: &str, path: &[&str], value: &str) -> anyhow::Result<String> {
    let toks = tokens(input)?;
    match find(&toks, path)? {
        Found::Key(k) => {
            let v = toks.get(k + 1).context("vdf ends after a key")?;
            anyhow::ensure!(
                v.kind == Kind::Str,
                "`{}` is a block, not a value",
                path.join("/")
            );
            Ok(format!(
                "{}{}{}",
                &input[..v.start],
                quote(value),
                &input[v.end..]
            ))
        }
        Found::Missing { depth, close } => {
            let tabs = |n: usize| "\t".repeat(n);
            let rest = &path[depth..];
            let mut add = String::new();
            for (n, part) in rest.iter().enumerate() {
                let indent = tabs(depth + n);
                if n + 1 == rest.len() {
                    add.push_str(&format!("{indent}{}\t\t{}\n", quote(part), quote(value)));
                } else {
                    add.push_str(&format!("{indent}{}\n{indent}{{\n", quote(part)));
                }
            }
            for n in (0..rest.len() - 1).rev() {
                add.push_str(&format!("{}}}\n", tabs(depth + n)));
            }
            Ok(match close {
                Some(c) => {
                    let at = line_start(input, toks[c].start);
                    // A `}` sharing its line with other text: start the new keys on a new line.
                    if input[at..toks[c].start].trim().is_empty() {
                        format!("{}{add}{}", &input[..at], &input[at..])
                    } else {
                        let at = toks[c].start;
                        format!("{}\n{add}{}", &input[..at], &input[at..])
                    }
                }
                None => {
                    let sep = if input.is_empty() || input.ends_with('\n') {
                        ""
                    } else {
                        "\n"
                    };
                    format!("{input}{sep}{add}")
                }
            })
        }
    }
}

/// Removes the key at `path` with its value (a string or a whole block). Missing: unchanged.
pub fn remove(input: &str, path: &[&str]) -> anyhow::Result<String> {
    let toks = tokens(input)?;
    let Found::Key(k) = find(&toks, path)? else {
        return Ok(input.to_string());
    };
    let last = skip_value(&toks, k + 1)? - 1;
    let from = line_start(input, toks[k].start);
    let mut to = toks[last].end;
    // Whole lines when the entry has its own lines; otherwise just the tokens.
    let own_lines = input[from..toks[k].start].trim().is_empty();
    let line_end = input[to..].find('\n').map_or(input.len(), |n| to + n + 1);
    if own_lines && input[to..line_end].trim().is_empty() {
        to = line_end;
        Ok(format!("{}{}", &input[..from], &input[to..]))
    } else {
        Ok(format!("{}{}", &input[..toks[k].start], &input[to..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"AutoUpdateWindowEnabled\"\t\t\"0\"\n\t\t\t}\n\t\t}\n\t}\n\t\"Music\"\t\t\"x\"\n}\n";

    #[test]
    fn sets_creates_and_removes_without_touching_the_rest() {
        let path = [
            "InstallConfigStore",
            "Software",
            "Valve",
            "Steam",
            "CompatToolMapping",
            "230410",
            "name",
        ];
        let out = set(CONFIG, &path, "GE-Proton10-25").unwrap();
        assert_eq!(get(&out, &path).unwrap().as_deref(), Some("GE-Proton10-25"));
        assert!(out.contains("\t\t\t\t\"CompatToolMapping\"\n\t\t\t\t{\n\t\t\t\t\t\"230410\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"name\"\t\t\"GE-Proton10-25\"\n"));
        assert!(
            out.contains("\"AutoUpdateWindowEnabled\"\t\t\"0\"")
                && out.contains("\"Music\"\t\t\"x\"")
        );
        // The parser agrees with the result.
        let doc = crate::vdf::parse(&out).unwrap();
        let steam = doc
            .obj("installconfigstore")
            .and_then(|o| o.obj("software"))
            .and_then(|o| o.obj("valve"))
            .and_then(|o| o.obj("steam"))
            .unwrap();
        assert_eq!(
            steam
                .obj("compattoolmapping")
                .and_then(|m| m.obj("230410"))
                .and_then(|a| a.str("name")),
            Some("GE-Proton10-25")
        );
        // Replacing a value changes only its token.
        let again = set(&out, &path, "proton_experimental").unwrap();
        assert_eq!(
            again.len(),
            out.len() - "GE-Proton10-25".len() + "proton_experimental".len()
        );
        // Case-insensitive keys; removing a block leaves the file as it was.
        let mapping = [
            "installconfigstore",
            "software",
            "valve",
            "steam",
            "compattoolmapping",
        ];
        assert_eq!(remove(&again, &mapping).unwrap(), CONFIG);
        assert_eq!(
            remove(CONFIG, &mapping).unwrap(),
            CONFIG,
            "missing: unchanged"
        );
    }

    #[test]
    fn escapes_quotes_in_values() {
        let path = ["UserLocalConfigStore", "apps", "730", "LaunchOptions"];
        let value = r#"'/home/u/.local/bin/arkor' run local:cs2 -- %command% -name "x y""#;
        let out = set(
            "\"UserLocalConfigStore\"\n{\n\t\"apps\"\n\t{\n\t}\n}\n",
            &path,
            value,
        )
        .unwrap();
        assert!(out.contains(r#"\"x y\""#));
        assert_eq!(get(&out, &path).unwrap().as_deref(), Some(value));
        assert!(
            set(&out, &["UserLocalConfigStore", "apps"], "x").is_err(),
            "a block is not a value"
        );
    }
}

#[cfg(test)]
mod real_files {
    /// Read-only: set + remove on the machine's own Steam files gives them back unchanged.
    #[test]
    #[ignore]
    fn round_trips_real_steam_files() {
        let home = dirs::home_dir().unwrap();
        let steam = home.join(".local/share/Steam");
        let mut files = vec![(
            steam.join("config/config.vdf"),
            vec![
                "InstallConfigStore",
                "Software",
                "Valve",
                "Steam",
                "CompatToolMapping",
                "999999990",
            ],
        )];
        for u in std::fs::read_dir(steam.join("userdata")).unwrap().flatten() {
            files.push((
                u.path().join("config/localconfig.vdf"),
                vec![
                    "UserLocalConfigStore",
                    "Software",
                    "Valve",
                    "Steam",
                    "apps",
                    "999999990",
                ],
            ));
        }
        for (file, block) in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let mut leaf = block.clone();
            leaf.push("name");
            let out = super::set(&text, &leaf, "x \"y\"").unwrap();
            crate::vdf::parse(&out).unwrap();
            assert_eq!(super::get(&out, &leaf).unwrap().as_deref(), Some("x \"y\""));
            // Remove the outermost block the set created (the shortest prefix not in `text`).
            let created = (1..=block.len())
                .find(|n| super::remove(&text, &block[..*n]).unwrap() == text)
                .unwrap();
            let back = super::remove(&out, &block[..created]).unwrap();
            if back != text {
                let at = back
                    .bytes()
                    .zip(text.bytes())
                    .position(|(a, b)| a != b)
                    .unwrap_or(back.len().min(text.len()));
                let ctx = |s: &str| s[at.saturating_sub(120)..(at + 80).min(s.len())].to_string();
                panic!(
                    "{} differs at {at} (len {} vs {}):\nGOT:{:?}\nWANT:{:?}\nSET:{:?}",
                    file.display(),
                    back.len(),
                    text.len(),
                    ctx(&back),
                    ctx(&text),
                    {
                        let i = out.find("999999990").unwrap();
                        out[i.saturating_sub(200)..(i + 120).min(out.len())].to_string()
                    }
                );
            }
            println!("ok {} ({} bytes)", file.display(), text.len());
        }
    }
}
