//! Minimal parser for Valve's text KeyValues format (`.vdf` / `.acf`).
//! Handles quoted and bare tokens, nested blocks, `//` comments and `\` escapes,
//! which covers libraryfolders.vdf and appmanifest files.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Obj(Obj),
}

/// Keys are lowercased: Valve's files are case-insensitive and mix cases between versions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Obj(pub BTreeMap<String, Value>);

impl Obj {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(&key.to_ascii_lowercase())
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            Value::Obj(_) => None,
        }
    }

    pub fn obj(&self, key: &str) -> Option<&Obj> {
        match self.get(key)? {
            Value::Obj(o) => Some(o),
            Value::Str(_) => None,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn tokenize(input: &str) -> anyhow::Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '{' => {
                chars.next();
                tokens.push(Token::Open);
            }
            '}' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '/' => {
                chars.next();
                anyhow::ensure!(chars.next() == Some('/'), "stray `/` in vdf");
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => anyhow::bail!("unterminated escape in vdf"),
                        },
                        Some(other) => s.push(other),
                        None => anyhow::bail!("unterminated string in vdf"),
                    }
                }
                tokens.push(Token::Str(s));
            }
            _ => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                        break;
                    }
                    s.push(c);
                    chars.next();
                }
                tokens.push(Token::Str(s));
            }
        }
    }
    Ok(tokens)
}

pub fn parse(input: &str) -> anyhow::Result<Obj> {
    let tokens = tokenize(input)?;
    let mut iter = tokens.into_iter();
    let obj = parse_obj(&mut iter, true)?;
    Ok(obj)
}

fn parse_obj(iter: &mut impl Iterator<Item = Token>, top_level: bool) -> anyhow::Result<Obj> {
    let mut obj = Obj::default();
    loop {
        let key = match iter.next() {
            Some(Token::Str(k)) => k.to_ascii_lowercase(),
            Some(Token::Close) if !top_level => return Ok(obj),
            None if top_level => return Ok(obj),
            other => anyhow::bail!("unexpected token in vdf: {other:?}"),
        };
        let value = match iter.next() {
            Some(Token::Str(v)) => Value::Str(v),
            Some(Token::Open) => Value::Obj(parse_obj(iter, false)?),
            other => anyhow::bail!("expected value for `{key}`, got {other:?}"),
        };
        obj.0.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested() {
        let obj = parse(
            r#"
            // comment
            "AppState"
            {
                "appid"     "730"
                "name"      "Counter-\"Strike\" 2"
                "InstalledDepots" { "731" { "manifest" "1" } }
            }
            "#,
        )
        .unwrap();
        let app = obj.obj("appstate").unwrap();
        assert_eq!(app.str("AppID"), Some("730"));
        assert_eq!(app.str("name"), Some("Counter-\"Strike\" 2"));
        assert!(app.obj("installeddepots").unwrap().obj("731").is_some());
    }

    #[test]
    fn rejects_unbalanced() {
        assert!(parse(r#""a" { "b" "c""#).is_err());
        assert!(parse(r#""a" "b" }"#).is_err());
    }
}
