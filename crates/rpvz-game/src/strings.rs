//! UI text by key: the game's own `properties/lawnstrings.txt` where it has
//! the key, otherwise RPvZ's own wording (for screens the original drew from
//! code, like naming a new player).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::library::Library;

/// RPvZ's own strings, used when the game data lacks a key.
const OWN: &[(&str, &str)] = &[
    ("RPVZ_NEW_PLAYER_HEADER", "New Player"),
    ("RPVZ_NEW_PLAYER_MESSAGE", "Type a name for your profile:"),
    ("RPVZ_RENAME_HEADER", "Change Name"),
    ("RPVZ_RENAME_MESSAGE", "Type a new name:"),
    ("DIALOG_BUTTON_OK", "OK"),
    ("DIALOG_BUTTON_CANCEL", "Cancel"),
    ("QUIT_HEADER", "Quit?"),
    ("QUIT_MESSAGE", "Do you want to quit?"),
    ("QUIT_BUTTON", "Quit"),
];

#[derive(Resource, Debug, Clone, Default)]
pub struct Strings {
    map: HashMap<String, String>,
}

impl Strings {
    pub fn load(library: &Library) -> Self {
        let mut map: HashMap<String, String> = OWN
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect();
        if let Some(path) = library.file("properties/lawnstrings.txt")
            && let Ok(bytes) = std::fs::read(&path)
        {
            map.extend(parse(&bytes));
        }
        Self { map }
    }

    /// Text for `key`, or the key itself in brackets if unknown.
    pub fn get(&self, key: &str) -> String {
        self.map
            .get(key)
            .cloned()
            .unwrap_or_else(|| format!("[{key}]"))
    }
}

/// `[KEY]` lines start entries; following lines (until the next key) are the
/// text. Latin-1, CRLF tolerated, trailing blank lines dropped.
pub fn parse(bytes: &[u8]) -> HashMap<String, String> {
    let text: String = bytes.iter().map(|&b| b as char).collect();
    let mut out = HashMap::new();
    let mut key: Option<String> = None;
    let mut lines: Vec<&str> = Vec::new();
    let mut flush = |key: &mut Option<String>, lines: &mut Vec<&str>| {
        if let Some(k) = key.take() {
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            out.insert(k, lines.join("\n"));
        }
        lines.clear();
    };
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.starts_with('[')
            && trimmed.ends_with(']')
            && trimmed.len() > 2
            && !trimmed.contains(' ')
        {
            flush(&mut key, &mut lines);
            key = Some(trimmed[1..trimmed.len() - 1].to_owned());
        } else if key.is_some() {
            lines.push(line);
        }
    }
    flush(&mut key, &mut lines);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiline_entries() {
        let m = parse(b"[A]\r\nHello\r\n\r\n[B]\nline one\nline two\n\n\n[C]\n");
        assert_eq!(m["A"], "Hello");
        assert_eq!(m["B"], "line one\nline two");
        assert_eq!(m["C"], "");
    }
}
