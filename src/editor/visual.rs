//! Visual mode (charwise, linewise, blockwise).

use super::normal::{Op, TextObj, VisualDot};
use super::operator::ORange;
use super::register::{RegKind, RegVal};
use super::{BlockInsert, Editor, Mode, VKind};
use crate::types::Pos;

impl Editor {
    pub fn visual_start(&mut self, kind: VKind, _count: Option<usize>) {
        match self.mode {
            Mode::Visual(k) if k == kind => self.exit_visual(),
            Mode::Visual(_) => self.mode = Mode::Visual(kind),
            _ => {
                let c = self.cursor();
                self.win_mut().vstart = c;
                self.mode = Mode::Visual(kind);
                self.messages.clear();
            }
        }
    }

    pub fn exit_visual(&mut self) {
        if let Mode::Visual(kind) = self.mode {
            let (a, b) = self.visual_bounds();
            self.buf_mut().marks.insert('<', a);
            self.buf_mut().marks.insert('>', b);
            self.win_mut().last_vkind = kind;
            self.mode = Mode::Normal;
            self.clamp_cursor();
        }
    }

    /// Ordered selection corners.
    pub fn visual_bounds(&self) -> (Pos, Pos) {
        let w = self.win();
        if w.vstart <= w.cursor { (w.vstart, w.cursor) } else { (w.cursor, w.vstart) }
    }

    pub fn visual_range(&self) -> ORange {
        let w = self.win();
        let b = self.buf();
        let (a, e) = self.visual_bounds();
        match self.mode {
            Mode::Visual(VKind::Line) => ORange::Lines(a.line, e.line),
            Mode::Visual(VKind::Block) => {
                let cc = w.want_col;
                let c1 = w.vstart.col.min(cc);
                let c2 = if cc == usize::MAX { usize::MAX } else { w.vstart.col.max(cc) };
                ORange::Block { l1: a.line, l2: e.line, c1, c2 }
            }
            _ => {
                let ai = b.pos_to_char(a);
                let ei = (b.pos_to_char(e) + 1).min(b.len_chars());
                ORange::Chars(ai, ei)
            }
        }
    }

    fn visual_dot_info(&self, r: ORange) -> VisualDot {
        let b = self.buf();
        match r {
            ORange::Lines(l1, l2) => VisualDot { kind: VKind::Line, lines: l2 - l1, cols: 0 },
            ORange::Block { l1, l2, c1, c2 } => {
                VisualDot { kind: VKind::Block, lines: l2 - l1, cols: c2.saturating_sub(c1).saturating_add(1).min(10_000) }
            }
            ORange::Chars(a, e) => {
                let pa = b.char_to_pos(a);
                let pe = b.char_to_pos(e.saturating_sub(1));
                if pa.line == pe.line {
                    VisualDot { kind: VKind::Char, lines: 0, cols: e - a }
                } else {
                    VisualDot { kind: VKind::Char, lines: pe.line - pa.line, cols: pe.col }
                }
            }
        }
    }

    pub fn visual_op(&mut self, op: Op, force_lines: bool, reg: Option<char>, n: usize) {
        let mut r = self.visual_range();
        if force_lines {
            let (a, e) = self.visual_bounds();
            r = match (self.mode, op) {
                // In block mode D/X/C/Y extend to the end of line.
                (Mode::Visual(VKind::Block), Op::Delete | Op::Change | Op::Yank) => {
                    let w = self.win();
                    ORange::Block { l1: a.line, l2: e.line, c1: w.vstart.col.min(w.want_col), c2: usize::MAX }
                }
                _ => ORange::Lines(a.line, e.line),
            };
        }
        self.last_visual_dot = Some(self.visual_dot_info(r));
        let (a, _) = self.visual_bounds();
        self.exit_visual();
        self.win_mut().cursor = a;
        let times = if matches!(op, Op::Indent | Op::Dedent) { n } else { 1 };
        self.apply_op(op, r, reg, times);
    }

    pub fn visual_obj(&mut self, obj: &TextObj, inner: bool, n: usize) {
        let Some(r) = self.text_object(obj, inner, n) else { return };
        let b = self.buf();
        match r {
            ORange::Chars(a, e) if e > a => {
                let (pa, pe) = (b.char_to_pos(a), b.char_to_pos(e - 1));
                let single = self.win().vstart == self.cursor();
                if single || pa < self.win().vstart {
                    self.win_mut().vstart = pa;
                }
                self.win_mut().cursor = pe;
                if self.mode == Mode::Visual(VKind::Line) {
                    self.mode = Mode::Visual(VKind::Char);
                }
            }
            ORange::Lines(l1, l2) => {
                let single = self.win().vstart == self.cursor();
                if single {
                    self.win_mut().vstart = Pos::new(l1, 0);
                }
                self.win_mut().cursor = Pos::new(l2, 0);
                self.mode = Mode::Visual(VKind::Line);
            }
            _ => {}
        }
        let c = self.cursor().col;
        self.win_mut().want_col = c;
    }

    pub fn visual_swap(&mut self, horizontal: bool) {
        let w = self.win_mut();
        if horizontal {
            let (vc, cc) = (w.vstart.col, w.cursor.col);
            w.vstart.col = cc;
            w.cursor.col = vc;
        } else {
            std::mem::swap(&mut w.vstart, &mut w.cursor);
        }
        w.want_col = w.cursor.col;
    }

    pub fn visual_block_insert(&mut self, append: bool) {
        let r = self.visual_range();
        let mode = self.mode;
        self.exit_visual();
        match (mode, r) {
            (Mode::Visual(VKind::Block), ORange::Block { l1, l2, c1, c2 }) => {
                let eol = c2 == usize::MAX;
                let col = if append { if eol { self.buf().line_len(l1) } else { c2 + 1 } } else { c1 };
                // Pad the first line if needed.
                let len = self.buf().line_len(l1);
                if len < col {
                    let st = self.buf().line_start(l1);
                    self.ins(st + len, &" ".repeat(col - len));
                }
                self.enter_insert(Pos::new(l1, col));
                self.insert.block = Some(BlockInsert { first: l1, last: l2, col, append_eol: append && eol, pad: append && !eol });
            }
            (_, ORange::Lines(l1, l2)) => {
                let p = if append { Pos::new(l2, self.buf().line_len(l2)) } else { Pos::new(l1, 0) };
                self.enter_insert(p);
            }
            (_, ORange::Chars(a, e)) => {
                let p = if append { self.buf().char_to_pos(e) } else { self.buf().char_to_pos(a) };
                self.enter_insert(p);
            }
            _ => {}
        }
    }

    pub fn visual_replace(&mut self, c: char) {
        let r = self.visual_range();
        let (a, _) = self.visual_bounds();
        self.exit_visual();
        let segs: Vec<(usize, usize)> = match r {
            ORange::Chars(s, e) => {
                // Per-line segments, skipping newlines.
                let b = self.buf();
                let (ps, pe) = (b.char_to_pos(s), b.char_to_pos(e.saturating_sub(1)));
                (ps.line..=pe.line)
                    .map(|l| {
                        let st = b.line_start(l);
                        let from = if l == ps.line { s } else { st };
                        let to = if l == pe.line { e.min(b.line_end(l)) } else { b.line_end(l) };
                        (from, to.max(from))
                    })
                    .collect()
            }
            ORange::Lines(l1, l2) => (l1..=l2).map(|l| (self.buf().line_start(l), self.buf().line_end(l))).collect(),
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
        for (s, e) in segs.into_iter().rev() {
            if e > s {
                self.del(s, e);
                self.ins(s, &c.to_string().repeat(e - s));
            }
        }
        self.set_cursor_col(a);
    }

    pub fn visual_join(&mut self, spaces: bool) {
        let (a, e) = self.visual_bounds();
        self.exit_visual();
        let joins = (e.line - a.line).max(1);
        self.join_lines(a.line, joins, spaces);
    }

    pub fn visual_put(&mut self, reg: Option<char>, swap: bool) {
        let r = reg.unwrap_or('"');
        let Some(val) = self.registers.get(r) else {
            self.error(format!("E353: Nothing in register {r}"));
            return;
        };
        let range = self.visual_range();
        self.exit_visual();
        let deleted = self.range_text(range);
        match range {
            ORange::Chars(a, e) => {
                self.del(a, e);
                let text = if val.kind == RegKind::Line { format!("\n{}\n", val.text) } else { val.text.clone() };
                self.ins(a, &text);
                let p = if val.kind == RegKind::Line {
                    let l = self.buf().char_to_pos(a).line + 1;
                    Pos::new(l, self.buf().first_non_blank(l))
                } else {
                    self.buf().char_to_pos(a + text.chars().count().saturating_sub(1))
                };
                self.set_cursor_col(p);
            }
            ORange::Lines(l1, l2) => {
                let at_end = l2 + 1 >= self.buf().line_count();
                self.delete_lines(l1, l2);
                let text = val.text.clone();
                if at_end && l1 > 0 {
                    let total = self.buf().len_chars();
                    self.ins(total, &format!("\n{text}"));
                } else if at_end {
                    // The whole buffer was replaced.
                    let total = self.buf().len_chars();
                    self.del(0, total);
                    self.ins(0, &text);
                } else {
                    let st = self.buf().line_start(l1);
                    self.ins(st, &format!("{text}\n"));
                }
                let c = self.buf().first_non_blank(l1);
                self.set_cursor_col(Pos::new(l1, c));
            }
            ORange::Block { l1, l2, c1, c2 } => {
                for l in (l1..=l2).rev() {
                    let len = self.buf().line_len(l);
                    if c1 < len {
                        let st = self.buf().line_start(l);
                        self.del(st + c1, st + c2.saturating_add(1).min(len));
                    }
                }
                self.win_mut().cursor = Pos::new(l1, c1);
                self.put_val(&val, true, false, 1);
            }
        }
        if swap {
            self.registers.delete(None, RegVal::new(deleted.text, deleted.kind));
        }
    }

    pub fn visual_incr(&mut self, delta: i64, progressive: bool) {
        let r = self.visual_range();
        let (a, _) = self.visual_bounds();
        self.exit_visual();
        let (l1, l2, c1, c2) = match r {
            ORange::Lines(l1, l2) => (l1, l2, 0, usize::MAX),
            ORange::Block { l1, l2, c1, c2 } => (l1, l2, c1, c2.saturating_add(1)),
            ORange::Chars(s, e) => {
                let b = self.buf();
                let (ps, pe) = (b.char_to_pos(s), b.char_to_pos(e.saturating_sub(1)));
                (ps.line, pe.line, ps.col, usize::MAX)
            }
        };
        let mut k = 0;
        for l in l1..=l2 {
            let d = if progressive { delta * (k + 1) } else { delta };
            let from = if l == l1 || matches!(r, ORange::Block { .. }) { c1 } else { 0 };
            if self.incr_in_line(l, from, c2, d).is_some() {
                k += 1;
            }
        }
        self.set_cursor_col(a);
    }

    pub fn reselect(&mut self) {
        let (Some(a), Some(b)) = (self.buf().marks.get(&'<').copied(), self.buf().marks.get(&'>').copied()) else {
            return;
        };
        let kind = self.win().last_vkind;
        self.mode = Mode::Visual(kind);
        self.win_mut().vstart = a;
        self.win_mut().cursor = b;
        self.clamp_cursor();
    }
}
