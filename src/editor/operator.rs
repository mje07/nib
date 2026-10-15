//! Operators applied to ranges, plus put/join/replace and friends.

use super::motion::MKind;
use super::normal::{Cmd, Motion, Op, Target, VisualDot};
use super::register::{RegKind, RegVal};
use super::{BlockInsert, Editor, Mode, VKind};
use crate::markdown::editing::parse_prefix;
use crate::types::Pos;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ORange {
    /// Char indices [a, b).
    Chars(usize, usize),
    /// Lines a..=b.
    Lines(usize, usize),
    /// Lines l1..=l2, char columns c1..=c2 (c2 = usize::MAX: to end of line).
    Block { l1: usize, l2: usize, c1: usize, c2: usize },
}

impl Editor {
    pub fn op_cmd(&mut self, op: Op, target: &Target, count: Option<usize>, reg: Option<char>, cmd: &Cmd) {
        let n = count.unwrap_or(1);
        let range = match target {
            Target::Lines => {
                // A closed fold counts as one line.
                let l = self.visible_line(self.buf(), self.cursor().line);
                let mut e = l;
                for _ in 1..n {
                    match self.next_visible(self.buf(), e) {
                        Some(x) => e = x,
                        None => break,
                    }
                }
                let e = self.closed_fold_at(self.buf(), e).map(|(_, fe)| fe).unwrap_or(e);
                Some(ORange::Lines(l, e))
            }
            Target::Obj(obj, inner) => self.text_object(obj, *inner, n),
            Target::Motion(Motion::Search(None, fwd)) => {
                self.cmdline.pending_cmd = Some(cmd.clone());
                self.start_search_prompt(*fwd);
                return;
            }
            Target::Motion(m) => self.motion_range(op, m, count),
            Target::Visual(vd) => Some(self.visual_dot_range(vd)),
        };
        let Some(r) = range else { return };
        self.apply_op(op, r, reg, 1);
    }

    fn visual_dot_range(&self, vd: &VisualDot) -> ORange {
        let cur = self.cursor();
        let b = self.buf();
        let l2 = (cur.line + vd.lines).min(b.line_count() - 1);
        match vd.kind {
            VKind::Line => ORange::Lines(cur.line, l2),
            VKind::Block => ORange::Block { l1: cur.line, l2, c1: cur.col, c2: cur.col + vd.cols.saturating_sub(1) },
            VKind::Char => {
                let a = b.pos_to_char(cur);
                if vd.lines == 0 {
                    ORange::Chars(a, (a + vd.cols).min(b.line_end(cur.line)))
                } else {
                    ORange::Chars(a, (b.pos_to_char(Pos::new(l2, vd.cols)) + 1).min(b.len_chars()))
                }
            }
        }
    }

    pub fn motion_range(&mut self, op: Op, m: &Motion, count: Option<usize>) -> Option<ORange> {
        let cur = self.cursor();
        let n = count.unwrap_or(1).max(1);
        // `cw` on a non-blank changes to the end of the word (like `ce`).
        if op == Op::Change
            && let Motion::WordFwd(big) = m
        {
            let b = self.buf();
            let idx = b.pos_to_char(cur);
            if b.char_at(idx).is_some_and(|c| !c.is_whitespace()) {
                let mut i = idx;
                for k in 0..n {
                    if k == 0 {
                        let c = super::motion::cls(b.text.char(i), *big);
                        while i + 1 < b.len_chars()
                            && b.text.char(i + 1) != '\n'
                            && super::motion::cls(b.text.char(i + 1), *big) == c
                        {
                            i += 1;
                        }
                    } else {
                        i = super::motion::next_word_end(b, i, *big);
                    }
                }
                return Some(ORange::Chars(idx, i + 1));
            }
        }
        let res = self.eval_motion(m, count, true)?;
        if res.jump {
            self.buf_mut().marks.insert('\'', cur);
        }
        let b = self.buf();
        let (s, e) = if res.pos < cur { (res.pos, cur) } else { (cur, res.pos) };
        Some(match res.kind {
            MKind::Linewise => ORange::Lines(s.line, e.line),
            MKind::Inclusive => {
                let si = b.pos_to_char(s);
                let ei = b.pos_to_char(e);
                let ei = if e.col < b.line_len(e.line) { ei + 1 } else { ei };
                ORange::Chars(si, ei)
            }
            MKind::Exclusive => {
                let mut e = e;
                if e.line > s.line && e.col == 0 {
                    if s.col <= b.first_non_blank(s.line) {
                        return Some(ORange::Lines(s.line, e.line - 1));
                    }
                    e = Pos::new(e.line - 1, b.line_len(e.line - 1));
                }
                ORange::Chars(b.pos_to_char(s), b.pos_to_char(e))
            }
        })
    }

    pub fn range_text(&self, r: ORange) -> RegVal {
        let b = self.buf();
        match r {
            ORange::Chars(a, e) => RegVal::new(b.slice(a, e), RegKind::Char),
            ORange::Lines(l1, l2) => {
                RegVal::new((l1..=l2).map(|l| b.line(l)).collect::<Vec<_>>().join("\n"), RegKind::Line)
            }
            ORange::Block { l1, l2, c1, c2 } => {
                let rows: Vec<String> = (l1..=l2)
                    .map(|l| {
                        let chars: Vec<char> = b.line(l).chars().collect();
                        let s = c1.min(chars.len());
                        let e = c2.saturating_add(1).min(chars.len());
                        chars[s..e.max(s)].iter().collect()
                    })
                    .collect();
                RegVal::new(rows.join("\n"), RegKind::Block)
            }
        }
    }

    pub fn delete_lines(&mut self, l1: usize, l2: usize) {
        let b = self.buf();
        let n = b.line_count();
        let (a, e) = if l2 + 1 < n {
            (b.line_start(l1), b.line_start(l2 + 1))
        } else if l1 > 0 {
            (b.line_end(l1 - 1), b.len_chars())
        } else {
            (0, b.len_chars())
        };
        self.del(a, e);
    }

    fn delete_block(&mut self, l1: usize, l2: usize, c1: usize, c2: usize) {
        for l in (l1..=l2).rev() {
            let len = self.buf().line_len(l);
            if c1 >= len {
                continue;
            }
            let st = self.buf().line_start(l);
            self.del(st + c1, st + c2.saturating_add(1).min(len));
        }
    }

    pub fn apply_op(&mut self, op: Op, r: ORange, reg: Option<char>, times: usize) {
        let (start, end) = self.range_bounds(r);
        self.buf_mut().marks.insert('[', start);
        self.buf_mut().marks.insert(']', end);
        match op {
            Op::Yank => {
                let val = self.range_text(r);
                let nlines = val.text.matches('\n').count() + 1;
                self.registers.yank(reg, val);
                match r {
                    ORange::Chars(a, _) => {
                        let p = self.buf().char_to_pos(a);
                        self.set_cursor_col(p);
                    }
                    ORange::Lines(l1, _) => {
                        if l1 < self.cursor().line {
                            let c = self.cursor().col;
                            self.set_cursor(Pos::new(l1, c));
                        }
                    }
                    ORange::Block { l1, c1, .. } => self.set_cursor_col(Pos::new(l1, c1)),
                }
                if nlines > 2 && matches!(r, ORange::Lines(..)) {
                    self.info(format!("{nlines} lines yanked"));
                }
            }
            Op::Delete => {
                let val = self.range_text(r);
                match r {
                    ORange::Chars(a, e) => {
                        if a >= e {
                            return;
                        }
                        self.del(a, e);
                        self.registers.delete(reg, val);
                        let p = self.buf().char_to_pos(a);
                        self.set_cursor_col(p);
                    }
                    ORange::Lines(l1, l2) => {
                        self.delete_lines(l1, l2);
                        self.registers.delete(reg, val);
                        let l = l1.min(self.buf().line_count() - 1);
                        let c = self.buf().first_non_blank(l);
                        self.set_cursor_col(Pos::new(l, c));
                        let n = l2 - l1 + 1;
                        if n > 2 {
                            self.info(format!("{n} fewer lines"));
                        }
                    }
                    ORange::Block { l1, l2, c1, c2 } => {
                        self.delete_block(l1, l2, c1, c2);
                        self.registers.delete(reg, val);
                        self.set_cursor_col(Pos::new(l1, c1));
                    }
                }
            }
            Op::Change => {
                let val = self.range_text(r);
                match r {
                    ORange::Chars(a, e) => {
                        self.del(a, e);
                        self.registers.delete(reg, val);
                        let p = self.buf().char_to_pos(a);
                        self.enter_insert(p);
                    }
                    ORange::Lines(l1, l2) => {
                        let indent = if self.options.autoindent { self.buf().indent_of(l1) } else { String::new() };
                        let (a, e) = (self.buf().line_start(l1), self.buf().line_end(l2));
                        self.del(a, e);
                        self.ins(a, &indent);
                        self.registers.delete(reg, val);
                        self.enter_insert(Pos::new(l1, indent.chars().count()));
                    }
                    ORange::Block { l1, l2, c1, c2 } => {
                        self.delete_block(l1, l2, c1, c2);
                        self.registers.delete(reg, val);
                        self.enter_insert(Pos::new(l1, c1));
                        self.insert.block = Some(BlockInsert { first: l1, last: l2, col: c1, append_eol: false, pad: false });
                    }
                }
            }
            Op::Indent | Op::Dedent => {
                let (l1, l2) = (start.line, end.line);
                self.shift_lines(l1, l2, op == Op::Indent, times);
                let c = self.buf().first_non_blank(l1);
                self.set_cursor_col(Pos::new(l1, c));
                let n = l2 - l1 + 1;
                if n > 2 {
                    self.info(format!("{} lines {}ed {} time{}", n, if op == Op::Indent { ">" } else { "<" }, times, if times == 1 { "" } else { "s" }));
                }
            }
            Op::Lower | Op::Upper | Op::Toggle | Op::Rot13 => {
                let f = |c: char| -> String {
                    match op {
                        Op::Lower => c.to_lowercase().collect(),
                        Op::Upper => c.to_uppercase().collect(),
                        Op::Toggle => {
                            if c.is_uppercase() {
                                c.to_lowercase().collect()
                            } else {
                                c.to_uppercase().collect()
                            }
                        }
                        _ => rot13(c).to_string(),
                    }
                };
                self.map_range(r, &f);
                match r {
                    ORange::Lines(l1, _) => {
                        let c = self.cursor().col;
                        self.set_cursor(Pos::new(l1, c));
                    }
                    _ => self.set_cursor_col(start),
                }
            }
            Op::Format | Op::FormatKeep => {
                let keep = self.cursor();
                let last = self.format_lines(start.line, end.line);
                if op == Op::FormatKeep {
                    self.set_cursor(keep);
                } else {
                    let c = self.buf().first_non_blank(last);
                    self.set_cursor_col(Pos::new(last, c));
                }
            }
        }
    }

    fn range_bounds(&self, r: ORange) -> (Pos, Pos) {
        let b = self.buf();
        match r {
            ORange::Chars(a, e) => (b.char_to_pos(a), b.char_to_pos(e.saturating_sub(1).max(a))),
            ORange::Lines(l1, l2) => (Pos::new(l1, 0), Pos::new(l2, b.line_len(l2).saturating_sub(1))),
            ORange::Block { l1, l2, c1, c2 } => (Pos::new(l1, c1), Pos::new(l2, c2.min(b.line_len(l2)))),
        }
    }

    fn map_range(&mut self, r: ORange, f: &dyn Fn(char) -> String) {
        let segments: Vec<(usize, usize)> = match r {
            ORange::Chars(a, e) => vec![(a, e)],
            ORange::Lines(l1, l2) => vec![(self.buf().line_start(l1), self.buf().line_end(l2))],
            ORange::Block { l1, l2, c1, c2 } => (l1..=l2)
                .filter_map(|l| {
                    let len = self.buf().line_len(l);
                    if c1 >= len {
                        return None;
                    }
                    let st = self.buf().line_start(l);
                    Some((st + c1, st + c2.saturating_add(1).min(len)))
                })
                .collect(),
        };
        for (a, e) in segments.into_iter().rev() {
            let old = self.buf().slice(a, e);
            let new: String = old.chars().map(f).collect();
            if new != old {
                self.del(a, e);
                self.ins(a, &new);
            }
        }
    }

    pub fn shift_lines(&mut self, l1: usize, l2: usize, right: bool, times: usize) {
        let sw = self.options.shiftwidth;
        let ts = self.options.tabstop;
        for l in l1..=l2 {
            let line = self.buf().line(l);
            if line.trim().is_empty() && right {
                continue;
            }
            let ind_chars: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            let width: usize = ind_chars.chars().fold(0, |w, c| if c == '\t' { w + ts - w % ts } else { w + 1 });
            let new_w = if right { width + sw * times } else { width.saturating_sub(sw * times) };
            let new_ind = self.indent_string(new_w);
            if new_ind != ind_chars {
                let st = self.buf().line_start(l);
                self.del(st, st + ind_chars.chars().count());
                self.ins(st, &new_ind);
            }
        }
    }

    pub fn indent_string(&self, width: usize) -> String {
        if self.options.expandtab {
            " ".repeat(width)
        } else {
            let ts = self.options.tabstop;
            format!("{}{}", "\t".repeat(width / ts), " ".repeat(width % ts))
        }
    }

    /// Reflow lines to textwidth. Returns the last line of the result.
    pub fn format_lines(&mut self, l1: usize, l2: usize) -> usize {
        let tw = if self.options.textwidth > 0 { self.options.textwidth } else { 79 };
        let src: Vec<String> = (l1..=l2).map(|l| self.buf().line(l)).collect();
        let special = |s: &str| {
            let t = s.trim_start();
            t.starts_with('#') || t.starts_with("```") || t.starts_with("~~~") || t.starts_with('|') || t == "---" || t == "***"
        };
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        let mut in_fence = false;
        while i < src.len() {
            let s = &src[i];
            let t = s.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                in_fence = !in_fence;
                out.push(s.clone());
                i += 1;
                continue;
            }
            if in_fence || s.trim().is_empty() || special(s) {
                out.push(s.clone());
                i += 1;
                continue;
            }
            let prefix = parse_prefix(s);
            let (lead, cont, body) = match &prefix {
                Some(p) => {
                    let lead: String = s.chars().take(p.len).collect();
                    let cont = if p.is_list() {
                        format!("{}{}", p.indent, " ".repeat(lead.chars().count() - p.indent.chars().count()))
                    } else {
                        lead.clone()
                    };
                    (lead.clone(), cont, s.chars().skip(p.len).collect::<String>())
                }
                None => {
                    let ind: String = s.chars().take_while(|c| c.is_whitespace()).collect();
                    let cont = if i + 1 < src.len() && !src[i + 1].trim().is_empty() && parse_prefix(&src[i + 1]).is_none() {
                        src[i + 1].chars().take_while(|c| c.is_whitespace()).collect()
                    } else {
                        ind.clone()
                    };
                    (ind, cont, s.trim_start().to_string())
                }
            };
            let mut words: Vec<String> = body.split_whitespace().map(String::from).collect();
            let mut j = i + 1;
            while j < src.len() {
                let n = &src[j];
                if n.trim().is_empty() || special(n) || parse_prefix(n).is_some_and(|p| p.is_list()) {
                    break;
                }
                let nb = match parse_prefix(n) {
                    Some(p) if !p.quote.is_empty() => n.chars().skip(p.len).collect::<String>(),
                    _ => n.clone(),
                };
                words.extend(nb.split_whitespace().map(String::from));
                j += 1;
            }
            let mut line = lead.clone();
            let mut has_word = false;
            for w in words {
                let lw = line.chars().count();
                if has_word && lw + 1 + w.chars().count() > tw {
                    out.push(line);
                    line = cont.clone();
                    has_word = false;
                }
                if has_word {
                    line.push(' ');
                }
                line.push_str(&w);
                has_word = true;
            }
            out.push(line);
            i = j;
        }
        let new = out.join("\n");
        let (a, e) = (self.buf().line_start(l1), self.buf().line_end(l2));
        if self.buf().slice(a, e) != new {
            self.del(a, e);
            self.ins(a, &new);
        }
        l1 + out.len() - 1
    }

    pub fn put(&mut self, reg: Option<char>, before: bool, gp: bool, count: usize) {
        let r = reg.unwrap_or('"');
        let Some(val) = self.registers.get(r) else {
            self.error(format!("E353: Nothing in register {r}"));
            return;
        };
        self.put_val(&val, before, gp, count);
    }

    pub fn put_val(&mut self, val: &RegVal, before: bool, gp: bool, count: usize) {
        let cur = self.cursor();
        match val.kind {
            RegKind::Char => {
                let text = val.text.repeat(count);
                let len = self.buf().line_len(cur.line);
                let base = self.buf().pos_to_char(cur);
                let at = if before || len == 0 { base } else { base + 1 };
                self.ins(at, &text);
                let end = at + text.chars().count();
                let p = if gp {
                    self.buf().char_to_pos(end)
                } else if text.contains('\n') {
                    self.buf().char_to_pos(at)
                } else {
                    self.buf().char_to_pos(end.saturating_sub(1))
                };
                if gp {
                    self.win_mut().cursor = p;
                    self.win_mut().want_col = p.col;
                } else {
                    self.set_cursor_col(p);
                }
                let start = self.buf().char_to_pos(at);
                self.buf_mut().marks.insert('[', start);
            }
            RegKind::Line => {
                let nl = val.text.matches('\n').count() + 1;
                let block = format!("{}\n", val.text).repeat(count);
                let line = if before { cur.line } else { cur.line + 1 };
                if line >= self.buf().line_count() {
                    let total = self.buf().len_chars();
                    self.ins(total, &format!("\n{}", &block[..block.len() - 1]));
                } else {
                    let st = self.buf().line_start(line);
                    self.ins(st, &block);
                }
                let total = nl * count;
                if gp {
                    let l = (line + total).min(self.buf().line_count() - 1);
                    self.set_cursor_col(Pos::new(l, 0));
                } else {
                    let c = self.buf().first_non_blank(line);
                    self.set_cursor_col(Pos::new(line, c));
                }
                if total > 2 {
                    self.info(format!("{total} more lines"));
                }
            }
            RegKind::Block => {
                let rows: Vec<&str> = val.text.split('\n').collect();
                let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
                let len = self.buf().line_len(cur.line);
                let col = if before || len == 0 { cur.col } else { cur.col + 1 };
                for (i, row) in rows.iter().enumerate() {
                    let l = cur.line + i;
                    if l >= self.buf().line_count() {
                        let total = self.buf().len_chars();
                        self.ins(total, "\n");
                    }
                    let ll = self.buf().line_len(l);
                    let st = self.buf().line_start(l);
                    if ll < col {
                        self.ins(st + ll, &" ".repeat(col - ll));
                    }
                    let mut text: String = row.repeat(count);
                    if ll > col {
                        let pad = width * count - text.chars().count();
                        text.push_str(&" ".repeat(pad));
                    }
                    self.ins(st + col, &text);
                }
                self.set_cursor_col(Pos::new(cur.line, col));
            }
        }
    }

    pub fn join_lines(&mut self, line: usize, joins: usize, spaces: bool) {
        for _ in 0..joins {
            if line + 1 >= self.buf().line_count() {
                break;
            }
            let cur_text = self.buf().line(line);
            let next = self.buf().line(line + 1);
            let end = self.buf().line_end(line);
            let cur_len = cur_text.chars().count();
            if spaces {
                let trimmed = next.trim_start();
                let lead = next.chars().count() - trimmed.chars().count();
                let sep = if trimmed.is_empty()
                    || cur_text.is_empty()
                    || cur_text.ends_with(' ')
                    || cur_text.ends_with('\t')
                    || trimmed.starts_with(')')
                {
                    ""
                } else {
                    " "
                };
                self.del(end, end + 1 + lead);
                self.ins(end, sep);
                self.win_mut().cursor = Pos::new(line, cur_len);
            } else {
                self.del(end, end + 1);
                self.win_mut().cursor = Pos::new(line, cur_len.saturating_sub(1));
            }
        }
        self.clamp_cursor();
        let c = self.cursor().col;
        self.win_mut().want_col = c;
    }

    pub fn tilde(&mut self, n: usize) {
        let cur = self.cursor();
        let len = self.buf().line_len(cur.line);
        if len == 0 {
            return;
        }
        let a = self.buf().pos_to_char(cur);
        let e = self.buf().line_start(cur.line) + (cur.col + n).min(len);
        self.map_range(ORange::Chars(a, e), &|c: char| {
            if c.is_uppercase() { c.to_lowercase().collect() } else { c.to_uppercase().collect() }
        });
        self.set_cursor_col(Pos::new(cur.line, (cur.col + n).min(len - 1)));
    }

    pub fn replace_chars(&mut self, c: char, n: usize) {
        let cur = self.cursor();
        let len = self.buf().line_len(cur.line);
        if cur.col + n > len {
            return;
        }
        let a = self.buf().pos_to_char(cur);
        self.del(a, a + n);
        if c == '\r' {
            self.ins(a, "\n");
            let ind = String::new();
            self.set_cursor_col(Pos::new(cur.line + 1, ind.len()));
        } else {
            self.ins(a, &c.to_string().repeat(n));
            self.set_cursor_col(Pos::new(cur.line, cur.col + n - 1));
        }
    }

    /// Ctrl-A / Ctrl-X on the number at or after the cursor.
    pub fn increment(&mut self, delta: i64) {
        let cur = self.cursor();
        if let Some(end) = self.incr_in_line(cur.line, cur.col, usize::MAX, delta) {
            self.set_cursor_col(Pos::new(cur.line, end));
        }
    }

    /// Increment the first number in `line` ending at or after `from`
    /// (and starting before `to`). Returns the column of its last char.
    pub fn incr_in_line(&mut self, line: usize, from: usize, to: usize, delta: i64) -> Option<usize> {
        let text = self.buf().line(line);
        let re = regex::Regex::new(r"0[xX][0-9a-fA-F]+|-?\d+").unwrap();
        for m in re.find_iter(&text) {
            let s = text[..m.start()].chars().count();
            let e = s + m.as_str().chars().count();
            if e <= from || s >= to {
                continue;
            }
            let t = m.as_str();
            // A '-' directly after a word char is not a sign.
            let (t, s) = if t.starts_with('-') && s > 0 && text[..m.start()].chars().last().is_some_and(|c| c.is_alphanumeric()) {
                (&t[1..], s + 1)
            } else {
                (t, s)
            };
            let new = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                let v = i64::from_str_radix(hex, 16).ok()?.wrapping_add(delta);
                let upper = hex.chars().any(|c| c.is_ascii_uppercase());
                let digits = if upper { format!("{:X}", v.max(0)) } else { format!("{:x}", v.max(0)) };
                format!("{}{:0>width$}", &t[..2], digits, width = hex.len())
            } else {
                let v: i64 = t.parse().ok()?;
                (v.saturating_add(delta)).to_string()
            };
            let st = self.buf().line_start(line);
            self.del(st + s, st + s + t.chars().count());
            self.ins(st + s, &new);
            return Some(s + new.chars().count() - 1);
        }
        None
    }

    /// Enter insert mode at `p` (used by `c`).
    pub fn enter_insert(&mut self, p: Pos) {
        self.mode = Mode::Insert;
        self.insert = super::InsertState { start: p, count: 1, ..Default::default() };
        self.messages.clear();
        self.win_mut().cursor = p;
        self.clamp_cursor();
        let c = self.cursor().col;
        self.win_mut().want_col = c;
    }
}

fn rot13(c: char) -> char {
    match c {
        'a'..='z' => (((c as u8 - b'a') + 13) % 26 + b'a') as char,
        'A'..='Z' => (((c as u8 - b'A') + 13) % 26 + b'A') as char,
        _ => c,
    }
}
