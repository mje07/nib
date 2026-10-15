//! Editor-level key representation, vim key-notation parsing (`<C-w>`, `<Esc>`)
//! and lossless encoding of keys into register strings (for macros).

use ratatui::crossterm::event::{KeyCode as CtKey, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyCode {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Tab,
    BackTab,
    Delete,
    Insert,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
}

impl Key {
    pub const fn new(code: KeyCode) -> Self {
        Key { code, ctrl: false, alt: false }
    }
    pub const fn ch(c: char) -> Self {
        Key::new(KeyCode::Char(c))
    }
    pub const fn ctrl(c: char) -> Self {
        Key { code: KeyCode::Char(c), ctrl: true, alt: false }
    }
    pub const ESC: Key = Key::new(KeyCode::Esc);
    pub const ENTER: Key = Key::new(KeyCode::Enter);

    /// The plain character for this key, if it is an unmodified character.
    pub fn char(&self) -> Option<char> {
        match self.code {
            KeyCode::Char(c) if !self.ctrl && !self.alt => Some(c),
            _ => None,
        }
    }

    pub fn is_ctrl(&self, c: char) -> bool {
        self.ctrl && !self.alt && self.code == KeyCode::Char(c)
    }

    pub fn from_event(ev: KeyEvent) -> Option<Key> {
        if ev.kind == KeyEventKind::Release {
            return None;
        }
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
        let alt = ev.modifiers.contains(KeyModifiers::ALT);
        let shift = ev.modifiers.contains(KeyModifiers::SHIFT);
        let code = match ev.code {
            CtKey::Char(c) => {
                // Normalise ctrl-letter to lowercase so <C-W> == <C-w>.
                let c = if ctrl && c.is_ascii_alphabetic() { c.to_ascii_lowercase() } else { c };
                KeyCode::Char(c)
            }
            CtKey::Esc => KeyCode::Esc,
            CtKey::Enter => KeyCode::Enter,
            CtKey::Backspace => KeyCode::Backspace,
            CtKey::Tab if shift => KeyCode::BackTab,
            CtKey::Tab => KeyCode::Tab,
            CtKey::BackTab => KeyCode::BackTab,
            CtKey::Delete => KeyCode::Delete,
            CtKey::Insert => KeyCode::Insert,
            CtKey::Up => KeyCode::Up,
            CtKey::Down => KeyCode::Down,
            CtKey::Left => KeyCode::Left,
            CtKey::Right => KeyCode::Right,
            CtKey::Home => KeyCode::Home,
            CtKey::End => KeyCode::End,
            CtKey::PageUp => KeyCode::PageUp,
            CtKey::PageDown => KeyCode::PageDown,
            CtKey::F(n) => KeyCode::F(n),
            _ => return None,
        };
        // Terminals send ctrl-[ as Esc, ctrl-h may arrive as Backspace etc.; keep as-is.
        Some(Key { code, ctrl, alt })
    }
}

/// Parse vim key notation into keys. `leader` replaces `<leader>`.
pub fn parse_keys(s: &str, leader: &str) -> Vec<Key> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<'
            && let Some(end) = chars[i + 1..].iter().position(|&c| c == '>') {
                let name: String = chars[i + 1..i + 1 + end].iter().collect();
                if name.eq_ignore_ascii_case("leader") {
                    out.extend(parse_keys(leader, ""));
                    i += end + 2;
                    continue;
                }
                if let Some(k) = parse_special(&name) {
                    out.push(k);
                    i += end + 2;
                    continue;
                }
            }
        out.push(Key::ch(chars[i]));
        i += 1;
    }
    out
}

fn parse_special(name: &str) -> Option<Key> {
    let lower = name.to_ascii_lowercase();
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut rest = lower.as_str();
    let mut orig_rest = name;
    loop {
        if rest.len() > 2 && rest.as_bytes()[1] == b'-' {
            match rest.as_bytes()[0] {
                b'c' => ctrl = true,
                b'a' | b'm' => alt = true,
                b's' => shift = true,
                _ => break,
            }
            rest = &rest[2..];
            orig_rest = &orig_rest[2..];
        } else {
            break;
        }
    }
    let code = match rest {
        "esc" => KeyCode::Esc,
        "cr" | "enter" | "return" => KeyCode::Enter,
        "bs" | "backspace" => KeyCode::Backspace,
        "tab" if shift => KeyCode::BackTab,
        "tab" => KeyCode::Tab,
        "del" | "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        "lt" => KeyCode::Char('<'),
        "bar" => KeyCode::Char('|'),
        "bslash" => KeyCode::Char('\\'),
        "nl" => KeyCode::Enter,
        r if r.len() > 1 && r.starts_with('f') && r[1..].parse::<u8>().is_ok() => {
            KeyCode::F(r[1..].parse().unwrap())
        }
        _ => {
            let mut cs = orig_rest.chars();
            let c = cs.next()?;
            if cs.next().is_some() || !(ctrl || alt || shift) {
                return None;
            }
            let c = if ctrl { c.to_ascii_lowercase() } else if shift { c.to_ascii_uppercase() } else { c };
            KeyCode::Char(c)
        }
    };
    // Normalise a couple of control aliases terminals actually send.
    if ctrl && code == KeyCode::Char('[') {
        return Some(Key::ESC);
    }
    Some(Key { code, ctrl, alt })
}

/// Render keys back to notation (for showcmd, :map listings).
pub fn keys_to_notation(keys: &[Key]) -> String {
    let mut s = String::new();
    for k in keys {
        s.push_str(&key_to_notation(k));
    }
    s
}

pub fn key_to_notation(k: &Key) -> String {
    let base = match k.code {
        KeyCode::Char(' ') if !k.ctrl && !k.alt => return " ".into(),
        KeyCode::Char('<') if !k.ctrl && !k.alt => return "<lt>".into(),
        KeyCode::Char(c) if !k.ctrl && !k.alt => return c.to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Enter => "CR".into(),
        KeyCode::Backspace => "BS".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => "S-Tab".into(),
        KeyCode::Delete => "Del".into(),
        KeyCode::Insert => "Insert".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PageUp".into(),
        KeyCode::PageDown => "PageDown".into(),
        KeyCode::F(n) => format!("F{n}"),
    };
    let mut s = String::from("<");
    if k.ctrl {
        s.push_str("C-");
    }
    if k.alt {
        s.push_str("M-");
    }
    s.push_str(&base);
    s.push('>');
    s
}

// --- Register encoding -----------------------------------------------------
// Macros are stored in registers as strings (like vim). Control keys map to
// ASCII control characters; everything else that has no ASCII form uses a
// private-use codepoint.

const PUA: u32 = 0xE000;

pub fn keys_to_register(keys: &[Key]) -> String {
    let mut s = String::new();
    for k in keys {
        let c = match (k.code, k.ctrl, k.alt) {
            (KeyCode::Char(c), false, false) => c,
            (KeyCode::Char(c), true, false) if c.is_ascii_lowercase() => {
                char::from_u32((c as u32) & 0x1f).unwrap()
            }
            (KeyCode::Esc, false, false) => '\x1b',
            (KeyCode::Enter, false, false) => '\r',
            (KeyCode::Tab, false, false) => '\t',
            (KeyCode::Backspace, false, false) => '\x7f',
            _ => {
                let idx = encode_special(k);
                char::from_u32(PUA + idx).unwrap_or('?')
            }
        };
        s.push(c);
    }
    s
}

pub fn register_to_keys(s: &str) -> Vec<Key> {
    s.chars()
        .map(|c| match c {
            '\x1b' => Key::ESC,
            '\r' | '\n' => Key::ENTER,
            '\t' => Key::new(KeyCode::Tab),
            '\x7f' | '\x08' => Key::new(KeyCode::Backspace),
            c if (c as u32) < 0x20 => Key::ctrl(char::from_u32(c as u32 | 0x60).unwrap()),
            c if (c as u32) >= PUA && (c as u32) < PUA + 0x1000 => decode_special(c as u32 - PUA),
            c => Key::ch(c),
        })
        .collect()
}

fn encode_special(k: &Key) -> u32 {
    let code = match k.code {
        KeyCode::Up => 1,
        KeyCode::Down => 2,
        KeyCode::Left => 3,
        KeyCode::Right => 4,
        KeyCode::Home => 5,
        KeyCode::End => 6,
        KeyCode::PageUp => 7,
        KeyCode::PageDown => 8,
        KeyCode::Delete => 9,
        KeyCode::Insert => 10,
        KeyCode::BackTab => 11,
        KeyCode::Esc => 12,
        KeyCode::Enter => 13,
        KeyCode::Tab => 14,
        KeyCode::Backspace => 15,
        KeyCode::F(n) => 32 + n as u32,
        // ctrl/alt + odd char: fall back to the character itself
        KeyCode::Char(c) => return (c as u32 & 0x7f) | 0x100 | if k.ctrl { 0x200 } else { 0 } | if k.alt { 0x400 } else { 0 },
    };
    code | if k.ctrl { 0x200 } else { 0 } | if k.alt { 0x400 } else { 0 }
}

fn decode_special(v: u32) -> Key {
    let ctrl = v & 0x200 != 0;
    let alt = v & 0x400 != 0;
    let code = if v & 0x100 != 0 {
        KeyCode::Char(char::from_u32(v & 0x7f).unwrap_or('?'))
    } else {
        match v & 0xff {
            1 => KeyCode::Up,
            2 => KeyCode::Down,
            3 => KeyCode::Left,
            4 => KeyCode::Right,
            5 => KeyCode::Home,
            6 => KeyCode::End,
            7 => KeyCode::PageUp,
            8 => KeyCode::PageDown,
            9 => KeyCode::Delete,
            10 => KeyCode::Insert,
            11 => KeyCode::BackTab,
            12 => KeyCode::Esc,
            13 => KeyCode::Enter,
            14 => KeyCode::Tab,
            15 => KeyCode::Backspace,
            n if n >= 32 => KeyCode::F((n - 32) as u8),
            _ => KeyCode::Esc,
        }
    };
    Key { code, ctrl, alt }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notation_roundtrip() {
        let keys = parse_keys("d<C-w>x<Esc><leader>w<CR><lt>", " ");
        assert_eq!(keys.len(), 8);
        assert_eq!(keys[1], Key::ctrl('w'));
        assert_eq!(keys[3], Key::ESC);
        assert_eq!(keys[4], Key::ch(' '));
        assert_eq!(keys[7], Key::ch('<'));
        let reg = keys_to_register(&keys);
        assert_eq!(register_to_keys(&reg), keys);
    }

    #[test]
    fn special_roundtrip() {
        let keys = vec![Key::new(KeyCode::Up), Key { code: KeyCode::Char('x'), ctrl: false, alt: true }];
        assert_eq!(register_to_keys(&keys_to_register(&keys)), keys);
    }
}
