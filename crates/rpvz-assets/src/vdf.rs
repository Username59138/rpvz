//! Minimal parser for Valve's text KeyValues format (`.vdf`, `.acf`).
//!
//! Supports quoted and bare tokens, escapes, `//` comments and `[$PLATFORM]`
//! conditionals (which are skipped). That is all Steam's library files use.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// Case-insensitive lookup of the first matching key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(items) => items
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Value::Str(_) => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Obj(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Value::Obj(items) => items,
            Value::Str(_) => &[],
        }
    }

    /// Follows a path of keys, e.g. `["AppState", "installdir"]`.
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        keys.iter().try_fold(self, |v, k| v.get(k))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
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
                if chars.peek() == Some(&'/') {
                    for c in chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                } else {
                    return Err("stray '/'".into());
                }
            }
            '[' => {
                // Platform conditional like [$WIN32]; it applies to the previous
                // pair and we don't care about it.
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                loop {
                    match chars.next() {
                        None => return Err("unterminated string".into()),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => return Err("unterminated escape".into()),
                        },
                        Some(other) => s.push(other),
                    }
                }
                tokens.push(Token::Str(s));
            }
            _ => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || matches!(c, '{' | '}' | '"') {
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

/// Parses a document into an object holding its top-level pairs.
pub fn parse(text: &str) -> Result<Value, String> {
    let tokens = tokenize(text.trim_start_matches('\u{feff}'))?;
    let mut iter = tokens.into_iter();
    let items = parse_object(&mut iter, true)?;
    Ok(Value::Obj(items))
}

fn parse_object(
    iter: &mut impl Iterator<Item = Token>,
    top_level: bool,
) -> Result<Vec<(String, Value)>, String> {
    let mut items = Vec::new();
    loop {
        let key = match iter.next() {
            None if top_level => return Ok(items),
            None => return Err("missing '}'".into()),
            Some(Token::Close) if !top_level => return Ok(items),
            Some(Token::Close) => return Err("unexpected '}'".into()),
            Some(Token::Open) => return Err("unexpected '{'".into()),
            Some(Token::Str(k)) => k,
        };
        let value = match iter.next() {
            Some(Token::Str(v)) => Value::Str(v),
            Some(Token::Open) => Value::Obj(parse_object(iter, false)?),
            _ => return Err(format!("key {key:?} has no value")),
        };
        items.push((key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_folders() {
        let text = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/u/.local/share/Steam"
		"apps"
		{
			"228980"		"123"
		}
	}
	"1" // second drive
	{
		"path"		"D:\\Games\\SteamLibrary"
		"apps" { "3590" "456" }
	}
}
"#;
        let v = parse(text).unwrap();
        let folders = v.get("LibraryFolders").unwrap();
        assert_eq!(folders.entries().len(), 2);
        assert_eq!(
            folders.path(&["1", "path"]).and_then(Value::as_str),
            Some(r"D:\Games\SteamLibrary")
        );
        assert!(folders.path(&["1", "apps", "3590"]).is_some());
    }

    #[test]
    fn skips_conditionals_and_bare_tokens() {
        let v = parse(r#"root { key value [$WIN32] other "x" }"#).unwrap();
        assert_eq!(
            v.path(&["root", "key"]).and_then(Value::as_str),
            Some("value")
        );
        assert_eq!(
            v.path(&["root", "other"]).and_then(Value::as_str),
            Some("x")
        );
    }

    #[test]
    fn rejects_broken_input() {
        assert!(parse(r#""a" { "b" "c" "#).is_err());
        assert!(parse(r#""a" }"#).is_err());
        assert!(parse(r#""a""#).is_err());
    }
}
