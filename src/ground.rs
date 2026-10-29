//! Ground materials: `.gt` ground types and the `materials.mtd` table that maps a
//! ground type's name to the materials painted on it.

use std::collections::HashMap;

/// Name of a ground type, the first line of a `.gt` file.
pub fn gt_name(bytes: &[u8]) -> Option<String> {
    let text = decode(bytes)?;
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

fn decode(b: &[u8]) -> Option<String> {
    let b = b.strip_prefix(&[0xff, 0xfe]).unwrap_or(b);
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16(&units).ok()
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Str(String),
    Num(u32),
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '"' {
            chars.next();
            let mut s = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                s.push(c);
            }
            out.push(Token::Str(s));
        } else if c.is_whitespace() {
            chars.next();
        } else {
            let mut w = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '"' {
                    break;
                }
                w.push(c);
                chars.next();
            }
            out.push(Token::Num(match w.as_str() {
                "True" | "true" => 1,
                "False" | "false" => 0,
                _ => w.parse().unwrap_or(0),
            }));
        }
    }
    out
}

/// Parse `materials.mtd`: ground type name (lower case) -> its heaviest material.
pub fn parse_mtd(bytes: &[u8]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(text) = decode(bytes) else { return out };
    let toks = tokenize(&text);
    // Skip "version N".
    let mut i = 0;
    if let Some(Token::Num(_)) | Some(Token::Str(_)) = toks.first() {
        // The version line is the words "version" and a number; the word became Num(0).
        i = 2;
    }
    let is_dlp = |t: &Token| matches!(t, Token::Str(s) if s.to_ascii_lowercase().ends_with(".dlp"));
    while i < toks.len() {
        let name = match &toks[i] {
            Token::Str(s) if !s.to_ascii_lowercase().ends_with(".mat") => {
                i += 1;
                Some(s.to_ascii_lowercase())
            }
            _ => None,
        };
        let (Some(Token::Num(a)), Some(Token::Num(b))) = (toks.get(i), toks.get(i + 1)) else { break };
        let (a, b) = (*a as usize, *b as usize);
        i += 2;
        let read_entries = |i: &mut usize, n: usize| -> Vec<String> {
            let mut mats = Vec::new();
            for _ in 0..n {
                if let Some(Token::Str(m)) = toks.get(*i) {
                    mats.push(m.replace('\\', "/"));
                    *i += 1;
                }
                while toks.get(*i).is_some_and(is_dlp) {
                    *i += 1;
                }
            }
            mats
        };
        let mats = read_entries(&mut i, a);
        let mut weights = Vec::new();
        if a > 1 {
            for _ in 0..=a {
                if let Some(Token::Num(w)) = toks.get(i) {
                    weights.push(*w);
                    i += 1;
                }
            }
        }
        if b > 0 {
            i += 2;
            read_entries(&mut i, b);
        }
        // Group without a name is the area default.
        let key = name.unwrap_or_default();
        let best = (0..mats.len()).max_by_key(|&k| weights.get(k).copied().unwrap_or(0));
        if let Some(k) = best {
            out.entry(key).or_insert_with(|| mats[k].clone());
        }
    }
    out
}
