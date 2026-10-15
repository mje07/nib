//! Insert and replace modes.

use super::{Completion, Dot, Editor, InsertState, Mode};
use crate::key::{Key, KeyCode};
use crate::markdown::editing::{Continue, Marker, continuation, next_prefix, parse_prefix, same_prefix};
use crate::types::Pos;

impl Editor {
    pub fn start_insert(&mut self, kind: char, count: usize) {
        let cur = self.cursor();
        let len = self.buf().line_len(cur.line);
        let pos = match kind {
            'a' => Pos::new(cur.line, if len == 0 { 0 } else { (cur.col + 1).min(len) }),
            'I' => Pos::new(cur.line, self.buf().first_non_blank(cur.line)),
            'G' => Pos::new(cur.line, 0),
            'A' => Pos::new(cur.line, len),
            'g' => self.buf().marks.get(&'^').copied().unwrap_or(cur),
            'o' | 'O' => self.open_line(kind == 'o'),
            _ => cur,
        };
        self.enter_insert(pos);
        self.insert.count = count.max(1);
        self.insert.open_kind = if kind == 'o' || kind == 'O' { Some(kind) } else { None };
    }

    pub fn start_replace(&mut self, count: usize) {
        let p = self.cursor();
        self.enter_insert(p);
        self.mode = Mode::Replace;
        self.insert.count = count.max(1);
    }

    /// Open a new line below/above the cursor line; returns the new cursor.
    fn open_line(&mut self, below: bool) -> Pos {
        let l = self.cursor().line;
        let text = self.buf().line(l);
        let mut prefix = if self.options.autoindent { self.buf().indent_of(l) } else { String::new() };
        if self.buf().is_markdown()
            && let Some(p) = parse_prefix(&text)
        {
            let rest: String = text.chars().skip(p.len).collect();
            prefix = if below {
                if rest.trim().is_empty() { format!("{}{}", p.indent, p.quote) } else { next_prefix(&p) }
            } else {
                same_prefix(&p)
            };
        }
        let plen = prefix.chars().count();
        if below {
            let end = self.buf().line_end(l);
            self.ins(end, &format!("\n{prefix}"));
            self.renumber_after(l + 1);
            Pos::new(l + 1, plen)
        } else {
            let st = self.buf().line_start(l);
            self.ins(st, &format!("{prefix}\n"));
            self.renumber_after(l);
            Pos::new(l, plen)
        }
    }

    pub fn insert_key(&mut self, key: Key) {
        if !self.insert.replaying {
            self.insert.keys.push(key);
        }
        if let Some(a) = self.insert.awaiting.take() {
            match a {
                'r' => {
                    if let Some(c) = key.char()
                        && let Some(v) = self.registers.get(c)
                    {
                        let text = if v.kind == super::register::RegKind::Line { format!("{}\n", v.text) } else { v.text };
                        self.insert_text(&text);
                    }
                }
                _ => {
                    let c = match key.code {
                        KeyCode::Char(c) if key.ctrl => char::from_u32((c as u32) & 0x1f),
                        KeyCode::Char(c) => Some(c),
                        KeyCode::Tab => Some('\t'),
                        KeyCode::Enter => Some('\r'),
                        KeyCode::Esc => Some('\x1b'),
                        _ => None,
                    };
                    if let Some(c) = c {
                        self.insert_char(c);
                    }
                }
            }
            return;
        }
        if self.insert.completion.is_some() {
            match key.code {
                KeyCode::Char('n') | KeyCode::Char('p') if key.ctrl => {
                    self.complete(key.code == KeyCode::Char('n'));
                    return;
                }
                KeyCode::Down | KeyCode::Up => {
                    self.complete(key.code == KeyCode::Down);
                    return;
                }
                KeyCode::Char('e') if key.ctrl => {
                    self.complete_cancel();
                    return;
                }
                KeyCode::Char('y') if key.ctrl => {
                    self.insert.completion = None;
                    return;
                }
                KeyCode::Enter if self.insert.completion.as_ref().is_some_and(|c| c.idx.is_some()) => {
                    self.insert.completion = None;
                    return;
                }
                _ => self.insert.completion = None,
            }
        }
        let cur = self.cursor();
        if key.ctrl {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('[') => self.leave_insert(),
                KeyCode::Char('h') => self.backspace(),
                KeyCode::Char('j') | KeyCode::Char('m') => self.insert_newline(),
                KeyCode::Char('w') => self.delete_word_back(),
                KeyCode::Char('u') => {
                    let fnb = self.buf().first_non_blank(cur.line);
                    let from = if cur.col > fnb { fnb } else { 0 };
                    let st = self.buf().line_start(cur.line);
                    self.del(st + from, st + cur.col);
                    self.set_cursor_col(Pos::new(cur.line, from));
                }
                KeyCode::Char('t') | KeyCode::Char('d') => {
                    let before = self.buf().line_len(cur.line);
                    self.shift_lines(cur.line, cur.line, key.code == KeyCode::Char('t'), 1);
                    let after = self.buf().line_len(cur.line);
                    let col = (cur.col as isize + after as isize - before as isize).max(0) as usize;
                    self.set_cursor_col(Pos::new(cur.line, col));
                }
                KeyCode::Char('r') => self.insert.awaiting = Some('r'),
                KeyCode::Char('v') | KeyCode::Char('q') => self.insert.awaiting = Some('v'),
                KeyCode::Char('o') => {
                    self.insert.ctrl_o = true;
                    self.mode = Mode::Normal;
                }
                KeyCode::Char('n') | KeyCode::Char('p') => self.complete(key.code == KeyCode::Char('n')),
                KeyCode::Char('e') | KeyCode::Char('y') => {
                    let other = if key.code == KeyCode::Char('e') { cur.line + 1 } else { cur.line.wrapping_sub(1) };
                    if other < self.buf().line_count()
                        && let Some(c) = self.buf().line(other).chars().nth(cur.col)
                    {
                        self.insert_char(c);
                    }
                }
                KeyCode::Char('a') => {
                    if let Some(v) = self.registers.get('.') {
                        self.insert_text(&v.text);
                    }
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.leave_insert(),
            KeyCode::Enter => self.insert_newline(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => {
                let idx = self.buf().pos_to_char(cur);
                if idx < self.buf().len_chars() {
                    self.del(idx, idx + 1);
                }
            }
            KeyCode::Left => {
                if cur.col > 0 {
                    self.set_cursor_col(Pos::new(cur.line, cur.col - 1));
                }
                self.insert.start = self.cursor();
            }
            KeyCode::Right => {
                self.set_cursor_col(Pos::new(cur.line, cur.col + 1));
                self.insert.start = self.cursor();
            }
            KeyCode::Up | KeyCode::Down => {
                let l = if key.code == KeyCode::Up { cur.line.saturating_sub(1) } else { cur.line + 1 };
                let l = l.min(self.buf().line_count() - 1);
                let col = self.col_for_want(l);
                self.win_mut().cursor = Pos::new(l, col);
                self.clamp_cursor();
                self.insert.start = self.cursor();
            }
            KeyCode::Home => self.set_cursor_col(Pos::new(cur.line, 0)),
            KeyCode::End => {
                let len = self.buf().line_len(cur.line);
                self.set_cursor_col(Pos::new(cur.line, len));
            }
            KeyCode::PageUp | KeyCode::PageDown => {}
            KeyCode::Insert => {
                self.mode = if self.mode == Mode::Replace { Mode::Insert } else { Mode::Replace };
            }
            KeyCode::Tab => self.insert_tab(),
            KeyCode::BackTab => {
                if self.list_line(cur.line) {
                    let before = self.buf().line_len(cur.line);
                    self.shift_lines(cur.line, cur.line, false, 1);
                    let after = self.buf().line_len(cur.line);
                    let col = (cur.col as isize + after as isize - before as isize).max(0) as usize;
                    self.set_cursor_col(Pos::new(cur.line, col));
                }
            }
            KeyCode::Char(c) if !key.alt => self.insert_char(c),
            _ => {}
        }
    }

    fn list_line(&self, l: usize) -> bool {
        self.buf().is_markdown() && parse_prefix(&self.buf().line(l)).is_some_and(|p| p.is_list())
    }

    fn insert_tab(&mut self) {
        let cur = self.cursor();
        if self.list_line(cur.line) {
            let before = self.buf().line_len(cur.line);
            self.shift_lines(cur.line, cur.line, true, 1);
            let after = self.buf().line_len(cur.line);
            self.set_cursor_col(Pos::new(cur.line, cur.col + after - before));
            return;
        }
        if self.options.expandtab {
            let line = self.buf().line(cur.line);
            let ts = self.options.tabstop.max(1);
            let vcol: usize = line.chars().take(cur.col).fold(0, |w, c| if c == '\t' { w + ts - w % ts } else { w + 1 });
            let n = ts - vcol % ts;
            self.insert_text(&" ".repeat(n));
        } else {
            self.insert_char('\t');
        }
    }

    pub fn insert_text(&mut self, s: &str) {
        let idx = self.buf().pos_to_char(self.cursor());
        self.ins(idx, s);
        let p = self.buf().char_to_pos(idx + s.chars().count());
        self.set_cursor_col(p);
    }

    fn insert_char(&mut self, c: char) {
        let cur = self.cursor();
        let idx = self.buf().pos_to_char(cur);
        if self.mode == Mode::Replace && cur.col < self.buf().line_len(cur.line) {
            let old = self.buf().char_at(idx);
            self.del(idx, idx + 1);
            self.insert.replaced.push(old);
        } else if self.mode == Mode::Replace {
            self.insert.replaced.push(None);
        }
        self.ins(idx, &c.to_string());
        self.set_cursor_col(Pos::new(cur.line, cur.col + 1));
    }

    fn backspace(&mut self) {
        let cur = self.cursor();
        if self.mode == Mode::Replace {
            if cur.col == 0 {
                return;
            }
            let idx = self.buf().pos_to_char(cur);
            match self.insert.replaced.pop() {
                Some(Some(orig)) => {
                    self.del(idx - 1, idx);
                    self.ins(idx - 1, &orig.to_string());
                }
                Some(None) => {
                    self.del(idx - 1, idx);
                }
                None => {}
            }
            self.set_cursor_col(Pos::new(cur.line, cur.col - 1));
            return;
        }
        let idx = self.buf().pos_to_char(cur);
        if cur.col > 0 {
            self.del(idx - 1, idx);
            self.set_cursor_col(Pos::new(cur.line, cur.col - 1));
        } else if cur.line > 0 {
            let prev_len = self.buf().line_len(cur.line - 1);
            self.del(idx - 1, idx);
            self.set_cursor_col(Pos::new(cur.line - 1, prev_len));
        }
    }

    fn delete_word_back(&mut self) {
        let cur = self.cursor();
        if cur.col == 0 {
            self.backspace();
            return;
        }
        let chars: Vec<char> = self.buf().line(cur.line).chars().collect();
        let mut c = cur.col;
        while c > 0 && chars[c - 1].is_whitespace() {
            c -= 1;
        }
        if c > 0 {
            let k = super::motion::cls(chars[c - 1], false);
            while c > 0 && super::motion::cls(chars[c - 1], false) == k {
                c -= 1;
            }
        }
        let st = self.buf().line_start(cur.line);
        self.del(st + c, st + cur.col);
        self.set_cursor_col(Pos::new(cur.line, c));
    }

    fn insert_newline(&mut self) {
        let cur = self.cursor();
        let line = self.buf().line(cur.line);
        let before: String = line.chars().take(cur.col).collect();
        let after: String = line.chars().skip(cur.col).collect();
        let mut prefix = if self.options.autoindent {
            before.chars().take_while(|c| *c == ' ' || *c == '\t').collect()
        } else {
            String::new()
        };
        if self.buf().is_markdown() {
            match continuation(&before) {
                Some(Continue::EndList) if after.trim().is_empty() => {
                    let p = parse_prefix(&before).unwrap();
                    // Empty item: end the list (an empty nested item outdents first).
                    let new = if p.is_list() && !p.indent.is_empty() {
                        let sw = self.options.shiftwidth;
                        let ind = p.indent.chars().count().saturating_sub(sw);
                        let mut np = p.clone();
                        np.indent = " ".repeat(ind);
                        crate::markdown::editing::same_prefix(&np)
                    } else {
                        p.indent.clone()
                    };
                    self.set_line(cur.line, &new);
                    let n = new.chars().count();
                    self.set_cursor_col(Pos::new(cur.line, n));
                    self.renumber_after(cur.line);
                    return;
                }
                Some(Continue::With(p)) => prefix = p,
                _ => {}
            }
        }
        let idx = self.buf().pos_to_char(cur);
        // An auto-indented line that stayed empty loses its indent.
        if before.trim().is_empty() && !before.is_empty() && self.insert.start.line == cur.line && after.is_empty() {
            let st = self.buf().line_start(cur.line);
            self.del(st, idx);
            let st = self.buf().line_start(cur.line);
            self.ins(st, &format!("\n{prefix}"));
            self.set_cursor_col(Pos::new(cur.line + 1, prefix.chars().count()));
            return;
        }
        self.ins(idx, &format!("\n{prefix}"));
        self.set_cursor_col(Pos::new(cur.line + 1, prefix.chars().count()));
        if prefix.chars().any(|c| c.is_ascii_digit()) {
            self.renumber_after(cur.line + 1);
        }
    }

    /// Renumber an ordered list containing `line` so items are sequential.
    pub fn renumber_after(&mut self, line: usize) {
        if !self.buf().is_markdown() || line >= self.buf().line_count() {
            return;
        }
        let Some(p) = parse_prefix(&self.buf().line(line)) else { return };
        let Marker::Ordered(..) = p.marker else { return };
        let indent = p.indent.clone();
        let item_at = |ed: &Editor, l: usize| -> Option<(u64, char, usize)> {
            let s = ed.buf().line(l);
            let q = parse_prefix(&s)?;
            match q.marker {
                Marker::Ordered(n, d) if q.indent == indent && q.quote == p.quote => {
                    let digits = n.to_string().len();
                    Some((n, d, q.indent.chars().count() + q.quote.chars().count() + digits))
                }
                _ => None,
            }
        };
        let nested = |ed: &Editor, l: usize| -> bool {
            let s = ed.buf().line(l);
            !s.trim().is_empty() && s.chars().take_while(|c| c.is_whitespace()).count() > indent.chars().count()
        };
        let mut start = line;
        while start > 0 && (item_at(self, start - 1).is_some() || nested(self, start - 1)) {
            start -= 1;
        }
        while item_at(self, start).is_none() && start < line {
            start += 1;
        }
        let Some((mut n, _, _)) = item_at(self, start) else { return };
        let cur = self.cursor();
        let mut l = start;
        while l < self.buf().line_count() {
            if let Some((old, _, end)) = item_at(self, l) {
                if old != n {
                    let st = self.buf().line_start(l);
                    let digits_start = end - old.to_string().len();
                    self.del(st + digits_start, st + end);
                    self.ins(st + digits_start, &n.to_string());
                    if l == cur.line {
                        let delta = n.to_string().len() as isize - old.to_string().len() as isize;
                        let col = (cur.col as isize + delta).max(0) as usize;
                        self.win_mut().cursor = Pos::new(l, col);
                    }
                }
                n += 1;
            } else if !nested(self, l) {
                break;
            }
            l += 1;
        }
    }

    pub fn leave_insert(&mut self) {
        self.insert.completion = None;
        let mut keys = std::mem::take(&mut self.insert.keys);
        if matches!(keys.last(), Some(k) if *k == Key::ESC || k.is_ctrl('c') || k.is_ctrl('[')) {
            keys.pop();
        }
        let count = self.insert.count;
        if count > 1 && !self.insert.replaying {
            let saved_mode = self.mode;
            for _ in 1..count {
                if let Some(k) = self.insert.open_kind {
                    let p = self.open_line(k == 'o');
                    self.set_cursor_col(p);
                }
                self.insert.replaying = true;
                for k in &keys {
                    self.insert_key(*k);
                }
                self.insert.replaying = false;
                self.mode = saved_mode;
            }
        }
        if let Some(bi) = self.insert.block.take() {
            let cur = self.cursor();
            if cur.line == bi.first && cur.col > bi.col {
                let line = self.buf().line(bi.first);
                let text: String = line.chars().skip(bi.col).take(cur.col - bi.col).collect();
                for l in bi.first + 1..=bi.last.min(self.buf().line_count() - 1) {
                    let len = self.buf().line_len(l);
                    let st = self.buf().line_start(l);
                    if bi.append_eol {
                        self.ins(st + len, &text);
                    } else if len >= bi.col {
                        self.ins(st + bi.col, &text);
                    } else if bi.pad {
                        // `A` pads short lines; `I` skips them.
                        self.ins(st + len, &format!("{}{}", " ".repeat(bi.col - len), text));
                    }
                }
            }
        }
        let typed = keys_text(&keys);
        self.registers.set_special('.', &typed);
        let cur = self.cursor();
        self.buf_mut().marks.insert('^', cur);
        self.mode = Mode::Normal;
        self.insert.ctrl_o = false;
        if cur.col > 0 {
            self.win_mut().cursor.col -= 1;
        }
        self.clamp_cursor();
        let c = self.cursor().col;
        self.win_mut().want_col = c;
        if !self.replaying_dot
            && let Some(cmd) = self.dot_pending.take()
        {
            self.dot = Some(Dot { cmd, insert_keys: Some(keys) });
        }
        self.insert = InsertState::default();
        self.commit();
    }

    fn complete(&mut self, forward: bool) {
        let cur = self.cursor();
        if self.insert.completion.is_none() {
            let chars: Vec<char> = self.buf().line(cur.line).chars().collect();
            let mut start = cur.col.min(chars.len());
            let is_kw = |c: char| c.is_alphanumeric() || c == '_';
            while start > 0 && is_kw(chars[start - 1]) {
                start -= 1;
            }
            let prefix: String = chars[start..cur.col.min(chars.len())].iter().collect();
            let items = self.completion_words(&prefix, forward);
            if items.is_empty() {
                self.error("-- Keyword completion: Pattern not found");
                return;
            }
            self.insert.completion = Some(Completion { start_col: start, prefix, items, idx: None });
        }
        let c = self.insert.completion.as_mut().unwrap();
        let n = c.items.len();
        c.idx = match (c.idx, forward) {
            (None, true) => Some(0),
            (None, false) => Some(n - 1),
            (Some(i), true) => if i + 1 < n { Some(i + 1) } else { None },
            (Some(i), false) => if i == 0 { None } else { Some(i - 1) },
        };
        let word = c.idx.map(|i| c.items[i].clone()).unwrap_or_else(|| c.prefix.clone());
        let start = c.start_col;
        let st = self.buf().line_start(cur.line);
        self.del(st + start, st + cur.col);
        self.ins(st + start, &word);
        self.set_cursor_col(Pos::new(cur.line, start + word.chars().count()));
    }

    fn complete_cancel(&mut self) {
        if let Some(c) = self.insert.completion.take() {
            let cur = self.cursor();
            let st = self.buf().line_start(cur.line);
            self.del(st + c.start_col, st + cur.col);
            self.ins(st + c.start_col, &c.prefix);
            self.set_cursor_col(Pos::new(cur.line, c.start_col + c.prefix.chars().count()));
        }
    }

    fn completion_words(&self, prefix: &str, forward: bool) -> Vec<String> {
        let re = regex::Regex::new(r"[\p{Alphabetic}\p{Nd}_]+").unwrap();
        let cur = self.cursor();
        let b = self.buf();
        let mut ordered: Vec<String> = Vec::new();
        let n = b.line_count();
        let lines: Vec<usize> = if forward {
            (cur.line..n).chain(0..cur.line).collect()
        } else {
            (0..=cur.line).rev().chain((cur.line + 1..n).rev()).collect()
        };
        let ic = self.options.ignorecase && !(self.options.smartcase && prefix.chars().any(|c| c.is_uppercase()));
        let lp = prefix.to_lowercase();
        let matches = |w: &str| {
            if w == prefix || w.chars().count() <= prefix.chars().count() {
                return false;
            }
            if ic { w.to_lowercase().starts_with(&lp) } else { w.starts_with(prefix) }
        };
        let mut seen = std::collections::HashSet::new();
        for l in lines {
            for m in re.find_iter(&b.line(l)) {
                let w = m.as_str();
                if matches(w) && seen.insert(w.to_string()) {
                    ordered.push(w.to_string());
                }
            }
            if ordered.len() >= 200 {
                break;
            }
        }
        for (id, ob) in &self.buffers {
            if *id == b.id {
                continue;
            }
            for m in re.find_iter(&ob.contents()) {
                let w = m.as_str();
                if matches(w) && seen.insert(w.to_string()) {
                    ordered.push(w.to_string());
                }
            }
        }
        ordered
    }
}

fn keys_text(keys: &[Key]) -> String {
    let mut s = String::new();
    for k in keys {
        match k.code {
            KeyCode::Char(c) if !k.ctrl && !k.alt => s.push(c),
            KeyCode::Enter => s.push('\n'),
            KeyCode::Tab => s.push('\t'),
            KeyCode::Backspace => {
                s.pop();
            }
            _ => {}
        }
    }
    s
}
