//! Motion evaluation.

use super::normal::Motion;
use super::Editor;
use crate::buffer::Buffer;
use crate::types::Pos;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MKind {
    Exclusive,
    Inclusive,
    Linewise,
}

#[derive(Clone, Copy, Debug)]
pub struct MotionRes {
    pub pos: Pos,
    pub kind: MKind,
    pub jump: bool,
    pub keep_want: bool,
    pub want_eol: bool,
}

impl MotionRes {
    fn new(pos: Pos, kind: MKind) -> Self {
        MotionRes { pos, kind, jump: false, keep_want: false, want_eol: false }
    }
    fn jump(mut self) -> Self {
        self.jump = true;
        self
    }
}

/// Character class for word motions: 0 blank, 1 punctuation, 2 word.
pub fn cls(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        0
    } else if big || !(c.is_alphanumeric() || c == '_') {
        1
    } else {
        2
    }
}

fn ch(b: &Buffer, i: usize) -> char {
    b.text.char(i)
}

fn empty_line_start(b: &Buffer, i: usize) -> bool {
    ch(b, i) == '\n' && (i == 0 || ch(b, i - 1) == '\n')
}

pub fn next_word_start(b: &Buffer, mut i: usize, big: bool) -> usize {
    let n = b.len_chars();
    if i >= n {
        return n;
    }
    let c = cls(ch(b, i), big);
    if c != 0 {
        while i < n && ch(b, i) != '\n' && cls(ch(b, i), big) == c {
            i += 1;
        }
    }
    while i < n {
        let c = ch(b, i);
        if c == '\n' {
            i += 1;
            if i < n && ch(b, i) == '\n' {
                return i;
            }
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        break;
    }
    i
}

pub fn prev_word_start(b: &Buffer, mut i: usize, big: bool) -> usize {
    if i == 0 {
        return 0;
    }
    i -= 1;
    while ch(b, i).is_whitespace() {
        if empty_line_start(b, i) {
            return i;
        }
        if i == 0 {
            return 0;
        }
        i -= 1;
    }
    let c = cls(ch(b, i), big);
    while i > 0 && ch(b, i - 1) != '\n' && cls(ch(b, i - 1), big) == c {
        i -= 1;
    }
    i
}

pub fn next_word_end(b: &Buffer, mut i: usize, big: bool) -> usize {
    let n = b.len_chars();
    if n == 0 {
        return 0;
    }
    i += 1;
    while i < n && ch(b, i).is_whitespace() {
        i += 1;
    }
    if i >= n {
        return n - 1;
    }
    let c = cls(ch(b, i), big);
    while i + 1 < n && ch(b, i + 1) != '\n' && cls(ch(b, i + 1), big) == c {
        i += 1;
    }
    i
}

pub fn prev_word_end(b: &Buffer, mut i: usize, big: bool) -> usize {
    if i == 0 || b.len_chars() == 0 {
        return 0;
    }
    let c = cls(ch(b, i.min(b.len_chars() - 1)), big);
    if c != 0 {
        while i > 0 && ch(b, i - 1) != '\n' && cls(ch(b, i - 1), big) == c {
            i -= 1;
        }
    }
    if i == 0 {
        return 0;
    }
    i -= 1;
    while ch(b, i).is_whitespace() {
        if empty_line_start(b, i) {
            return i;
        }
        if i == 0 {
            return 0;
        }
        i -= 1;
    }
    i
}

fn is_sentence_end(b: &Buffer, i: usize) -> bool {
    // i points just after the end punctuation (+ closing chars).
    let n = b.len_chars();
    i >= n || ch(b, i).is_whitespace()
}

/// Start of the next sentence after `i`.
pub fn next_sentence(b: &Buffer, mut i: usize) -> usize {
    let n = b.len_chars();
    let start_line_empty = i < n && empty_line_start(b, i);
    if start_line_empty {
        // Skip the blank lines; the next paragraph starts a sentence.
        while i < n && ch(b, i) == '\n' {
            i += 1;
        }
        return i;
    }
    while i < n {
        let c = ch(b, i);
        if c == '\n' && i + 1 < n && ch(b, i + 1) == '\n' {
            return i + 1;
        }
        if matches!(c, '.' | '!' | '?') {
            let mut j = i + 1;
            while j < n && matches!(ch(b, j), ')' | ']' | '"' | '\'') {
                j += 1;
            }
            if is_sentence_end(b, j) {
                while j < n && ch(b, j).is_whitespace() {
                    if ch(b, j) == '\n' && j + 1 < n && ch(b, j + 1) == '\n' {
                        return j + 1;
                    }
                    j += 1;
                }
                return j.min(n);
            }
        }
        i += 1;
    }
    n
}

/// Start of the sentence containing `i` (or the previous one if at a start).
pub fn prev_sentence(b: &Buffer, i: usize) -> usize {
    if i == 0 {
        return 0;
    }
    // Find sentence starts before i by scanning from the paragraph start.
    let mut p = i - 1;
    while p > 0 && !(ch(b, p) == '\n' && ch(b, p - 1) == '\n') {
        p -= 1;
    }
    let mut s = if p == 0 { 0 } else { p + 1 };
    if s >= i {
        // We were at the paragraph start: go to the previous paragraph's last sentence.
        return if p == 0 { 0 } else { prev_sentence(b, p) };
    }
    let mut last = s;
    while s < i {
        last = s;
        let nx = next_sentence(b, s);
        if nx <= s {
            break;
        }
        s = nx;
    }
    last
}

impl Editor {
    pub fn eval_motion(&mut self, m: &Motion, count: Option<usize>, for_op: bool) -> Option<MotionRes> {
        use MKind::*;
        let n = count.unwrap_or(1).max(1);
        let cur = self.cursor();
        let b = self.buf();
        let lines = b.line_count();
        let len = b.line_len(cur.line);
        let idx = b.pos_to_char(cur);
        let total = b.len_chars();
        let to_pos = |b: &Buffer, i: usize, for_op: bool| {
            if !for_op && i >= b.len_chars() && b.len_chars() > 0 {
                let p = b.char_to_pos(b.len_chars());
                Pos::new(p.line, p.col.saturating_sub(1))
            } else {
                b.char_to_pos(i)
            }
        };
        let res = match m {
            Motion::Left => {
                if cur.col == 0 {
                    return None;
                }
                MotionRes::new(Pos::new(cur.line, cur.col.saturating_sub(n)), Exclusive)
            }
            Motion::Right => {
                if len == 0 || (!for_op && cur.col + 1 >= len) {
                    return None;
                }
                let max = if for_op { len } else { len - 1 };
                MotionRes::new(Pos::new(cur.line, (cur.col + n).min(max)), Exclusive)
            }
            Motion::Down | Motion::Up | Motion::GDown | Motion::GUp => {
                let down = matches!(m, Motion::Down | Motion::GDown);
                let mut l = cur.line;
                let mut moved = false;
                for _ in 0..n {
                    let nx = if down { self.next_visible(b, l) } else { self.prev_visible(b, l) };
                    match nx {
                        Some(x) => {
                            l = x;
                            moved = true;
                        }
                        None => break,
                    }
                }
                if !moved {
                    return None;
                }
                if for_op && down {
                    // Include the whole of a closed fold at the end.
                    if let Some((_, e)) = self.closed_fold_at(b, l) {
                        l = e;
                    }
                }
                let mut r = MotionRes::new(Pos::new(l, self.col_for_want(l)), Linewise);
                r.keep_want = true;
                r
            }
            Motion::LineStart => MotionRes::new(Pos::new(cur.line, 0), Exclusive),
            Motion::FirstNonBlank => MotionRes::new(Pos::new(cur.line, b.first_non_blank(cur.line)), Exclusive),
            Motion::LineEnd => {
                let l = (cur.line + n - 1).min(lines - 1);
                let ll = b.line_len(l);
                let mut r = MotionRes::new(Pos::new(l, ll.saturating_sub(1)), Inclusive);
                r.want_eol = true;
                r
            }
            Motion::LastNonBlank => {
                let l = (cur.line + n - 1).min(lines - 1);
                let s = b.line(l);
                let c = s.trim_end().chars().count().saturating_sub(1);
                MotionRes::new(Pos::new(l, c), Inclusive)
            }
            Motion::Column => MotionRes::new(Pos::new(cur.line, (n - 1).min(len.saturating_sub(1))), Exclusive),
            Motion::WordFwd(big) => {
                let mut i = idx;
                let mut prev = i;
                for _ in 0..n {
                    prev = i;
                    i = next_word_start(b, i, *big);
                }
                let mut pos = to_pos(b, i, for_op);
                if for_op {
                    // The last word moved over ends the operated text at its line end.
                    let pp = b.char_to_pos(prev);
                    if pos.line > pp.line && i <= total {
                        let ppl = pp.line;
                        if b.line_len(ppl) > 0 || ppl == cur.line {
                            pos = Pos::new(ppl, b.line_len(ppl));
                        }
                    }
                }
                MotionRes::new(pos, Exclusive)
            }
            Motion::WordBack(big) => {
                let mut i = idx;
                for _ in 0..n {
                    i = prev_word_start(b, i, *big);
                }
                if i == idx && idx == 0 && !for_op {
                    return None;
                }
                MotionRes::new(b.char_to_pos(i), Exclusive)
            }
            Motion::WordEnd(big) => {
                let mut i = idx;
                for _ in 0..n {
                    i = next_word_end(b, i, *big);
                }
                MotionRes::new(b.char_to_pos(i), Inclusive)
            }
            Motion::WordEndBack(big) => {
                let mut i = idx;
                for _ in 0..n {
                    i = prev_word_end(b, i, *big);
                }
                MotionRes::new(b.char_to_pos(i), Inclusive)
            }
            Motion::Find { ch, fwd, till } => {
                self.last_find = Some((*ch, *fwd, *till));
                let col = find_in_line(&self.buf().line(cur.line), cur.col, *ch, *fwd, *till, n, false)?;
                MotionRes::new(Pos::new(cur.line, col), if *fwd { Inclusive } else { Exclusive })
            }
            Motion::RepeatFind(rev) => {
                let (c, fwd, till) = self.last_find?;
                let fwd = fwd != *rev;
                let col = find_in_line(&b.line(cur.line), cur.col, c, fwd, till, n, true)?;
                MotionRes::new(Pos::new(cur.line, col), if fwd { Inclusive } else { Exclusive })
            }
            Motion::GotoLine | Motion::GotoFirst => {
                let l = match (m, count) {
                    (_, Some(c)) => c.saturating_sub(1).min(lines - 1),
                    (Motion::GotoLine, None) => lines - 1,
                    _ => 0,
                };
                // Neovim's default 'nostartofline': keep the desired column.
                let mut r = MotionRes::new(Pos::new(l, self.col_for_want(l)), Linewise).jump();
                r.keep_want = true;
                r
            }
            Motion::Percent => {
                let l = ((n * lines).div_ceil(100)).saturating_sub(1).min(lines - 1);
                MotionRes::new(Pos::new(l, b.first_non_blank(l)), Linewise).jump()
            }
            Motion::MatchPair => {
                let i = match_pair(b, cur)?;
                MotionRes::new(b.char_to_pos(i), Inclusive).jump()
            }
            Motion::ParaFwd | Motion::ParaBack => {
                let fwd = *m == Motion::ParaFwd;
                let mut l = cur.line;
                for _ in 0..n {
                    // Skip blank lines, then non-blank lines.
                    if fwd {
                        while l + 1 < lines && b.line_len(l) == 0 {
                            l += 1;
                        }
                        while l + 1 < lines && b.line_len(l) != 0 {
                            l += 1;
                        }
                    } else {
                        while l > 0 && b.line_len(l) == 0 {
                            l -= 1;
                        }
                        while l > 0 && b.line_len(l) != 0 {
                            l -= 1;
                        }
                    }
                }
                let pos = if fwd && b.line_len(l) != 0 {
                    // Hit the end of the buffer.
                    if for_op { Pos::new(l, b.line_len(l)) } else { Pos::new(l, b.line_len(l).saturating_sub(1)) }
                } else {
                    Pos::new(l, 0)
                };
                let mut r = MotionRes::new(pos, Exclusive).jump();
                if fwd && b.line_len(l) != 0 && for_op {
                    r.kind = Exclusive;
                }
                r
            }
            Motion::SentFwd => {
                let mut i = idx;
                for _ in 0..n {
                    i = next_sentence(b, i);
                }
                MotionRes::new(to_pos(b, i, for_op), Exclusive).jump()
            }
            Motion::SentBack => {
                let mut i = idx;
                for _ in 0..n {
                    i = prev_sentence(b, i);
                }
                MotionRes::new(b.char_to_pos(i), Exclusive).jump()
            }
            Motion::SectionFwd | Motion::SectionBack => {
                let fwd = *m == Motion::SectionFwd;
                let is_section = |l: usize| -> bool {
                    if b.is_markdown() {
                        b.markdown().heading_level(l).is_some()
                    } else {
                        b.line(l).starts_with('{')
                    }
                };
                let mut l = cur.line;
                for _ in 0..n {
                    if fwd {
                        l += 1;
                        while l < lines && !is_section(l) {
                            l += 1;
                        }
                        if l >= lines {
                            l = lines - 1;
                            break;
                        }
                    } else {
                        if l == 0 {
                            break;
                        }
                        l -= 1;
                        while l > 0 && !is_section(l) {
                            l -= 1;
                        }
                    }
                }
                MotionRes::new(Pos::new(l, 0), Exclusive).jump()
            }
            Motion::ScreenTop | Motion::ScreenMid | Motion::ScreenBottom => {
                let vis = self.visible_line_list();
                if vis.is_empty() {
                    return None;
                }
                let so = self.options.scrolloff.min(vis.len().saturating_sub(1) / 2);
                let b = self.buf();
                let l = match m {
                    Motion::ScreenTop => {
                        let min = if vis[0] == 0 { 0 } else { so };
                        vis[(n - 1).max(min).min(vis.len() - 1)]
                    }
                    Motion::ScreenMid => vis[(vis.len() - 1) / 2],
                    _ => {
                        let last = *vis.last().unwrap();
                        let min = if last + 1 >= b.line_count() { 0 } else { so };
                        vis[vis.len().saturating_sub(1 + (n - 1).max(min))]
                    }
                };
                MotionRes::new(Pos::new(l, b.first_non_blank(l)), Linewise).jump()
            }
            Motion::SearchNext(rev) => {
                let Some(pat) = self.search.pattern.clone() else {
                    self.error("E35: No previous regular expression");
                    return None;
                };
                let fwd = self.search.forward != *rev;
                self.search.hl = true;
                let p = self.search_n(&pat, cur, fwd, n)?;
                MotionRes::new(p, Exclusive).jump()
            }
            Motion::Star { fwd } => {
                let (word, start) = self.word_under_cursor()?;
                let is_word = word.chars().all(|c| c.is_alphanumeric() || c == '_');
                let esc = regex::escape(&word);
                let pat = if is_word { format!("\\b{esc}\\b") } else { esc };
                self.search.pattern = Some(pat.clone());
                self.search.forward = *fwd;
                self.search.hl = true;
                self.registers.set_special('/', &pat);
                self.search_history.push(pat.clone());
                // Search from the word start so `#` skips the current word.
                let from = Pos::new(cur.line, start);
                let p = self.search_n_literal_case(&pat, from, *fwd, n)?;
                MotionRes::new(p, Exclusive).jump()
            }
            Motion::Search(pat, fwd) => {
                let pat = pat.clone()?;
                let p = self.search_n(&pat, cur, *fwd, n)?;
                MotionRes::new(p, Exclusive).jump()
            }
            Motion::Mark { ch, line } => {
                let (bid, p) = self.get_mark(*ch)?;
                if bid != self.win().buf {
                    if for_op {
                        return None;
                    }
                    self.push_jump();
                    self.switch_buffer(bid);
                }
                let b = self.buf();
                let p = Pos::new(p.line.min(b.line_count() - 1), p.col);
                if *line {
                    MotionRes::new(Pos::new(p.line, b.first_non_blank(p.line)), Linewise).jump()
                } else {
                    MotionRes::new(p, Exclusive).jump()
                }
            }
            Motion::NextLineStart | Motion::PrevLineStart | Motion::CurLineStart => {
                let l = match m {
                    Motion::NextLineStart => {
                        if cur.line + 1 >= lines {
                            return None;
                        }
                        (cur.line + n).min(lines - 1)
                    }
                    Motion::PrevLineStart => {
                        if cur.line == 0 {
                            return None;
                        }
                        cur.line.saturating_sub(n)
                    }
                    _ => (cur.line + n - 1).min(lines - 1),
                };
                MotionRes::new(Pos::new(l, b.first_non_blank(l)), Linewise)
            }
        };
        Some(res)
    }

    pub fn do_motion(&mut self, m: &Motion, count: Option<usize>) {
        if let Motion::Search(None, fwd) = m {
            self.start_search_prompt(*fwd);
            return;
        }
        let Some(res) = self.eval_motion(m, count, false) else { return };
        if res.jump {
            self.push_jump();
        }
        self.win_mut().cursor = res.pos;
        if res.want_eol {
            self.win_mut().want_col = usize::MAX;
            self.clamp_cursor();
        } else if !res.keep_want {
            self.clamp_cursor();
            let c = self.cursor().col;
            self.win_mut().want_col = c;
            let l = self.cursor().line;
            if self.closed_fold_at(self.buf(), l).is_some_and(|(s, _)| s != l) {
                // Horizontal motions, searches and marks open folds ('foldopen').
                self.open_folds_at(l);
            }
        }
        // Linewise jumps into a closed fold land on the fold line.
        let l = self.cursor().line;
        let v = self.visible_line(self.buf(), l);
        if v != l {
            self.win_mut().cursor.line = v;
        }
        self.clamp_cursor();
    }

    /// The keyword under or after the cursor and its start column.
    pub fn word_under_cursor(&self) -> Option<(String, usize)> {
        let line: Vec<char> = self.buf().line(self.cursor().line).chars().collect();
        let mut c = self.cursor().col;
        let is_kw = |ch: char| ch.is_alphanumeric() || ch == '_';
        while c < line.len() && !is_kw(line[c]) {
            c += 1;
        }
        if c >= line.len() {
            // No keyword: use the non-blank WORD under the cursor.
            let c = self.cursor().col;
            if c >= line.len() || line[c].is_whitespace() {
                return None;
            }
            let mut a = c;
            while a > 0 && !line[a - 1].is_whitespace() {
                a -= 1;
            }
            let mut e = c;
            while e < line.len() && !line[e].is_whitespace() {
                e += 1;
            }
            return Some((line[a..e].iter().collect(), a));
        }
        let mut a = c;
        while a > 0 && is_kw(line[a - 1]) {
            a -= 1;
        }
        let mut e = c;
        while e < line.len() && is_kw(line[e]) {
            e += 1;
        }
        Some((line[a..e].iter().collect(), a))
    }
}

fn find_in_line(line: &str, col: usize, c: char, fwd: bool, till: bool, n: usize, repeat: bool) -> Option<usize> {
    let chars: Vec<char> = line.chars().collect();
    let mut pos = col as isize;
    // When repeating a till motion, skip the adjacent match.
    let skip = if till && repeat { 1 } else { 0 };
    for i in 0..n {
        let start = if fwd { pos + 1 + if i == 0 { skip } else { 0 } } else { pos - 1 - if i == 0 { skip } else { 0 } };
        let mut j = start;
        let mut found = None;
        while j >= 0 && (j as usize) < chars.len() {
            if chars[j as usize] == c {
                found = Some(j);
                break;
            }
            j += if fwd { 1 } else { -1 };
        }
        pos = found?;
    }
    let p = if till { if fwd { pos - 1 } else { pos + 1 } } else { pos };
    Some(p as usize)
}

pub fn match_pair(b: &Buffer, cur: Pos) -> Option<usize> {
    let line: Vec<char> = b.line(cur.line).chars().collect();
    let pairs = [('(', ')'), ('[', ']'), ('{', '}')];
    let mut col = cur.col;
    while col < line.len() && !pairs.iter().any(|(o, c)| line[col] == *o || line[col] == *c) {
        col += 1;
    }
    if col >= line.len() {
        return None;
    }
    let c = line[col];
    let (open, close, fwd) = pairs
        .iter()
        .find_map(|(o, cl)| if c == *o { Some((*o, *cl, true)) } else if c == *cl { Some((*o, *cl, false)) } else { None })?;
    let start = b.line_start(cur.line) + col;
    let mut depth = 0i32;
    let n = b.len_chars();
    let mut i = start as isize;
    while i >= 0 && (i as usize) < n {
        let ch = b.text.char(i as usize);
        if ch == open {
            depth += if fwd { 1 } else { -1 };
        } else if ch == close {
            depth += if fwd { -1 } else { 1 };
        }
        if depth == 0 {
            return Some(i as usize);
        }
        i += if fwd { 1 } else { -1 };
    }
    None
}
