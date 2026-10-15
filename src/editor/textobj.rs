//! Text objects (`iw`, `a(`, `ip`, ...).

use super::Editor;
use super::motion::{cls, next_sentence, prev_sentence};
use super::normal::TextObj;
use super::operator::ORange;
use crate::buffer::Buffer;

impl Editor {
    pub fn text_object(&self, obj: &TextObj, inner: bool, count: usize) -> Option<ORange> {
        let b = self.buf();
        let cur = self.cursor();
        let count = count.max(1);
        match obj {
            TextObj::Word(big) => word_obj(b, cur.line, cur.col, *big, inner, count),
            TextObj::Sentence => {
                let idx = b.pos_to_char(cur);
                let start = prev_sentence(b, idx + 1);
                let mut end = start;
                for _ in 0..count {
                    end = next_sentence(b, end);
                }
                if inner {
                    while end > start && b.char_at(end - 1).is_some_and(|c| c.is_whitespace()) {
                        end -= 1;
                    }
                }
                Some(ORange::Chars(start, end))
            }
            TextObj::Paragraph => para_obj(b, cur.line, inner, count),
            TextObj::Quote(q) => quote_obj(b, cur.line, cur.col, *q, inner),
            TextObj::Block(o, c) => block_obj(b, b.pos_to_char(cur), *o, *c, inner, count),
            TextObj::Tag => tag_obj(b, b.pos_to_char(cur), inner),
        }
    }
}

fn word_obj(b: &Buffer, line: usize, col: usize, big: bool, inner: bool, count: usize) -> Option<ORange> {
    let chars: Vec<char> = b.line(line).chars().collect();
    let base = b.line_start(line);
    if chars.is_empty() {
        return None;
    }
    let col = col.min(chars.len() - 1);
    let class = |i: usize| cls(chars[i], big);
    // Extent of the run containing i.
    let run = |i: usize| -> (usize, usize) {
        let c = class(i);
        let mut a = i;
        while a > 0 && class(a - 1) == c {
            a -= 1;
        }
        let mut e = i + 1;
        while e < chars.len() && class(e) == c {
            e += 1;
        }
        (a, e)
    };
    let (start, mut end) = run(col);
    let on_space = class(col) == 0;
    if inner {
        for _ in 1..count {
            if end >= chars.len() {
                break;
            }
            end = run(end).1;
        }
        return Some(ORange::Chars(base + start, base + end));
    }
    let mut start = start;
    if on_space {
        // Whitespace plus the following word.
        if end < chars.len() {
            end = run(end).1;
        }
    } else if end < chars.len() && class(end) == 0 {
        end = run(end).1;
    } else {
        // No trailing space: take leading whitespace instead.
        if start > 0 && class(start - 1) == 0 {
            start = run(start - 1).0;
        }
    }
    for _ in 1..count {
        if end >= chars.len() {
            break;
        }
        end = run(end).1;
        if end < chars.len() && class(end) == 0 {
            end = run(end).1;
        }
    }
    Some(ORange::Chars(base + start, base + end))
}

fn para_obj(b: &Buffer, line: usize, inner: bool, count: usize) -> Option<ORange> {
    let n = b.line_count();
    let blank = |l: usize| b.line(l).trim().is_empty();
    let block_end = |l: usize| {
        let k = blank(l);
        let mut e = l;
        while e + 1 < n && blank(e + 1) == k {
            e += 1;
        }
        e
    };
    let k = blank(line);
    let mut start = line;
    while start > 0 && blank(start - 1) == k {
        start -= 1;
    }
    let mut end = block_end(line);
    let mut blocks = if inner { count } else { count * 2 };
    blocks -= 1;
    while blocks > 0 && end + 1 < n {
        end = block_end(end + 1);
        blocks -= 1;
    }
    if !inner && blocks > 0 && !k {
        // No trailing blank lines: include preceding ones.
        while start > 0 && blank(start - 1) {
            start -= 1;
        }
    }
    Some(ORange::Lines(start, end))
}

fn quote_obj(b: &Buffer, line: usize, col: usize, q: char, inner: bool) -> Option<ORange> {
    let chars: Vec<char> = b.line(line).chars().collect();
    let base = b.line_start(line);
    let qs: Vec<usize> = (0..chars.len()).filter(|&i| chars[i] == q && (i == 0 || chars[i - 1] != '\\')).collect();
    let pairs: Vec<(usize, usize)> = qs.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect();
    let (a, e) = pairs
        .iter()
        .find(|(a, e)| *a <= col && col <= *e)
        .or_else(|| pairs.iter().find(|(a, _)| *a > col))
        .copied()?;
    if inner {
        return Some(ORange::Chars(base + a + 1, base + e));
    }
    let mut s = a;
    let mut t = e + 1;
    if t < chars.len() && chars[t].is_whitespace() {
        while t < chars.len() && chars[t].is_whitespace() {
            t += 1;
        }
    } else {
        while s > 0 && chars[s - 1].is_whitespace() {
            s -= 1;
        }
    }
    Some(ORange::Chars(base + s, base + t))
}

fn block_obj(b: &Buffer, idx: usize, open: char, close: char, inner: bool, count: usize) -> Option<ORange> {
    let n = b.len_chars();
    let chr = |i: usize| b.text.char(i);
    // Find the enclosing open bracket `count` levels out.
    let mut o = None;
    let mut i = idx as isize;
    let mut levels = count;
    let mut depth = 0i32;
    if idx < n && chr(idx) == close {
        depth = 1;
        i -= 1;
    }
    while i >= 0 {
        let c = chr(i as usize);
        if c == close {
            depth += 1;
        } else if c == open {
            if depth == 0 {
                levels -= 1;
                if levels == 0 {
                    o = Some(i as usize);
                    break;
                }
            } else {
                depth -= 1;
            }
        }
        i -= 1;
    }
    let o = o?;
    let mut depth = 0;
    let mut cl = None;
    for j in o + 1..n {
        let c = chr(j);
        if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                cl = Some(j);
                break;
            }
            depth -= 1;
        }
    }
    let cl = cl?;
    if !inner {
        return Some(ORange::Chars(o, cl + 1));
    }
    let mut s = o + 1;
    let mut e = cl;
    let ol = b.char_to_pos(o).line;
    let cll = b.char_to_pos(cl).line;
    if ol != cll {
        if s < n && chr(s) == '\n' {
            s += 1;
        }
        let close_line_start = b.line_start(cll);
        if b.slice(close_line_start, cl).trim().is_empty() {
            e = close_line_start;
            if ol + 1 < cll && s == b.line_start(ol + 1) {
                return Some(ORange::Lines(ol + 1, cll - 1));
            }
            if e > s {
                e -= 1;
            }
        }
    }
    Some(ORange::Chars(s, e.max(s)))
}

fn tag_obj(b: &Buffer, idx: usize, inner: bool) -> Option<ORange> {
    let text = b.contents();
    let re = regex::Regex::new(r"<(/?)([A-Za-z][\w:.-]*)[^>]*?(/?)>").ok()?;
    let byte_idx = b.text.char_to_byte(idx);
    let mut stack: Vec<(String, usize, usize)> = Vec::new();
    let mut best: Option<(usize, usize, usize, usize)> = None;
    for cap in re.captures_iter(&text) {
        let m = cap.get(0)?;
        let closing = !cap[1].is_empty();
        let selfclose = !cap[3].is_empty();
        let name = cap[2].to_string();
        if selfclose {
            continue;
        }
        if !closing {
            stack.push((name, m.start(), m.end()));
        } else if let Some(pos) = stack.iter().rposition(|(n, _, _)| *n == name) {
            let (_, os, oe) = stack[pos].clone();
            stack.truncate(pos);
            if os <= byte_idx && byte_idx < m.end() {
                let cand = (os, oe, m.start(), m.end());
                // Innermost: the latest-starting pair containing the cursor.
                if best.is_none_or(|bb| cand.0 >= bb.0) {
                    best = Some(cand);
                }
            }
        }
    }
    let (os, oe, cs, ce) = best?;
    let c = |byte: usize| b.text.byte_to_char(byte);
    Some(if inner { ORange::Chars(c(oe), c(cs)) } else { ORange::Chars(c(os), c(ce)) })
}
