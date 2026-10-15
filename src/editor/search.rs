//! Regex search (`/`, `?`, `n`, `*`) and highlighting support.

use regex::{Regex, RegexBuilder};

use super::Editor;
use crate::types::Pos;

/// Translate a few vim-isms into Rust regex syntax.
pub fn translate(pat: &str) -> (String, Option<bool>) {
    let mut out = String::new();
    let mut case = None;
    let mut chars = pat.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('<') | Some('>') => {
                    chars.next();
                    out.push_str("\\b");
                }
                Some('c') => {
                    chars.next();
                    case = Some(true);
                }
                Some('C') => {
                    chars.next();
                    case = Some(false);
                }
                Some(_) => {
                    out.push(c);
                    out.push(chars.next().unwrap());
                }
                None => out.push_str("\\\\"),
            }
        } else {
            out.push(c);
        }
    }
    (out, case)
}

impl Editor {
    pub fn build_regex(&self, pat: &str, smart: bool) -> Result<Regex, String> {
        let (p, case) = translate(pat);
        let ignore = case.unwrap_or_else(|| {
            self.options.ignorecase && !(smart && self.options.smartcase && p.chars().any(|c| c.is_uppercase()))
        });
        RegexBuilder::new(&p)
            .case_insensitive(ignore)
            .multi_line(true)
            .build()
            .map_err(|e| format!("E486: Invalid pattern: {}", e.to_string().lines().last().unwrap_or("")))
    }

    /// Find the next match strictly after (or before) `from`.
    /// Returns (start, end) char indices and whether the search wrapped.
    pub fn find_match(&self, re: &Regex, from: usize, fwd: bool, wrap: bool) -> Option<(usize, usize, bool)> {
        let b = self.buf();
        let text = b.contents();
        let from_byte = b.text.char_to_byte(from.min(b.len_chars()));
        let to_chars = |m: regex::Match| (b.text.byte_to_char(m.start()), b.text.byte_to_char(m.end()));
        if fwd {
            let start = if from_byte < text.len() {
                from_byte + text[from_byte..].chars().next().map(|c| c.len_utf8()).unwrap_or(1)
            } else {
                text.len()
            };
            if start <= text.len()
                && let Some(m) = re.find_at(&text, start)
            {
                let (s, e) = to_chars(m);
                return Some((s, e, false));
            }
            if wrap {
                let m = re.find(&text)?;
                let (s, e) = to_chars(m);
                return Some((s, e, true));
            }
            None
        } else {
            let mut last = None;
            let mut overall = None;
            for m in re.find_iter(&text) {
                if m.start() < from_byte {
                    last = Some(m);
                }
                overall = Some(m);
            }
            if let Some(m) = last {
                let (s, e) = to_chars(m);
                return Some((s, e, false));
            }
            if wrap && let Some(m) = overall {
                let (s, e) = to_chars(m);
                return Some((s, e, true));
            }
            None
        }
    }

    fn search_impl(&mut self, pat: &str, from: Pos, fwd: bool, n: usize, smart: bool) -> Option<Pos> {
        let re = match self.build_regex(pat, smart) {
            Ok(r) => r,
            Err(e) => {
                self.error(e);
                return None;
            }
        };
        let mut idx = self.buf().pos_to_char(from);
        let mut wrapped = false;
        for _ in 0..n {
            match self.find_match(&re, idx, fwd, self.options.wrapscan) {
                Some((s, _, w)) => {
                    idx = s;
                    wrapped |= w;
                }
                None => {
                    if self.options.wrapscan {
                        self.error(format!("E486: Pattern not found: {pat}"));
                    } else {
                        self.error(format!("E38{}: Search hit {} without match for: {pat}", if fwd { 5 } else { 4 }, if fwd { "BOTTOM" } else { "TOP" }));
                    }
                    return None;
                }
            }
        }
        if wrapped {
            self.error(if fwd { "search hit BOTTOM, continuing at TOP" } else { "search hit TOP, continuing at BOTTOM" });
        } else {
            self.info(format!("{}{}", if fwd { '/' } else { '?' }, pat));
        }
        Some(self.buf().char_to_pos(idx))
    }

    pub fn search_n(&mut self, pat: &str, from: Pos, fwd: bool, n: usize) -> Option<Pos> {
        self.search_impl(pat, from, fwd, n, true)
    }

    /// `*`/`#` ignore smartcase.
    pub fn search_n_literal_case(&mut self, pat: &str, from: Pos, fwd: bool, n: usize) -> Option<Pos> {
        self.search_impl(pat, from, fwd, n, false)
    }

    pub fn start_search_prompt(&mut self, fwd: bool) {
        self.start_cmdline(if fwd { '/' } else { '?' }, "");
    }

    /// Match ranges (char columns) in a line, for highlighting.
    pub fn line_matches(&self, re: &Regex, line: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for m in re.find_iter(line) {
            if m.start() == m.end() {
                continue;
            }
            let s = line[..m.start()].chars().count();
            let e = s + line[m.start()..m.end()].chars().count();
            out.push((s, e));
        }
        out
    }

    /// Regex used to highlight matches, if hlsearch is active.
    pub fn highlight_regex(&self) -> Option<Regex> {
        if !self.options.hlsearch || !self.search.hl {
            return None;
        }
        let pat = self.search.pattern.as_ref()?;
        self.build_regex(pat, true).ok()
    }
}
