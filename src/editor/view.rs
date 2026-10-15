//! Window viewport: which lines are rendered how, soft wrapping, folds,
//! scrolling and screen ↔ buffer position mapping.

use std::ops::Range;

use super::{Editor, Window};
use crate::buffer::Buffer;
use crate::markdown::render::{Cell, RenderMode, RenderedLine, render_line, wrap};
use crate::types::{Pos, WinId};

pub struct LineView {
    pub line: usize,
    pub rl: RenderedLine,
    pub rows: Vec<Range<usize>>,
    /// Last line of a closed fold starting here.
    pub fold_end: Option<usize>,
    pub raw: bool,
    /// Heading drawn with the terminal text-sizing protocol.
    pub scale: Option<Scale>,
}

/// Kitty text-sizing parameters: block of `s`×`s` cells, font scaled by `n/d`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scale {
    pub s: u8,
    pub n: u8,
    pub d: u8,
}

impl Scale {
    /// Font size relative to normal text.
    pub fn factor(&self) -> f32 {
        if self.n == 0 || self.d == 0 { self.s as f32 } else { self.s as f32 * self.n as f32 / self.d as f32 }
    }

    /// Obsidian-like sizes: H1 2×, H2 1.75×, H3 1.5×, H4 1.25×; H5/H6 normal.
    pub fn for_heading(level: u8) -> Option<Scale> {
        Some(match level {
            1 => Scale { s: 2, n: 0, d: 0 },
            2 => Scale { s: 2, n: 7, d: 8 },
            3 => Scale { s: 2, n: 3, d: 4 },
            4 => Scale { s: 2, n: 5, d: 8 },
            _ => return None,
        })
    }
}

impl LineView {
    /// Screen rows of one wrapped row.
    pub fn row_h(&self) -> usize {
        self.scale.map(|s| s.s as usize).unwrap_or(1)
    }

    /// Total screen rows the line occupies.
    pub fn height(&self) -> usize {
        self.rows.len() * self.row_h()
    }

    /// Wrapped-row index for a screen row offset within the line.
    pub fn row_at(&self, rel: usize) -> usize {
        (rel / self.row_h()).min(self.rows.len().saturating_sub(1))
    }
}

pub const FOLD_FLAG: u32 = 1 << 23;

impl Editor {
    /// Inclusive range of lines shown as raw source in a previewed window.
    pub fn reveal_range(&self, win: &Window) -> (usize, usize) {
        let buf = &self.buffers[&win.buf];
        let c = win.cursor.line;
        let is_active = win.id == self.win_id();
        let (mut a, mut b) = (c, c);
        if is_active && self.mode.is_visual() {
            a = a.min(win.vstart.line);
            b = b.max(win.vstart.line);
        }
        if buf.is_markdown() {
            let doc = buf.markdown();
            if let Some((s, _)) = doc.group(a) {
                a = a.min(s);
            }
            if let Some((_, e)) = doc.group(b) {
                b = b.max(e);
            }
        }
        (a, b)
    }

    pub fn render_mode(&self, win: &Window, line: usize, reveal: (usize, usize)) -> RenderMode {
        let buf = &self.buffers[&win.buf];
        if !buf.is_markdown() {
            RenderMode::Plain
        } else if !self.preview_on(win) || (line >= reveal.0 && line <= reveal.1) {
            RenderMode::Raw
        } else {
            RenderMode::Preview
        }
    }

    pub fn render_buf_line(&self, buf: &Buffer, line: usize, mode: RenderMode) -> RenderedLine {
        let text = buf.line(line);
        if mode == RenderMode::Plain {
            render_line(None, &text, line, mode, self.options.tabstop)
        } else {
            let doc = buf.markdown();
            render_line(Some(&doc), &text, line, mode, self.options.tabstop)
        }
    }

    // --- Folds -----------------------------------------------------------------

    /// Last line of the section headed at `start`, if `start` is a heading.
    pub fn fold_end(&self, buf: &Buffer, start: usize) -> Option<usize> {
        if !buf.is_markdown() {
            return None;
        }
        buf.markdown().section_ends.get(&start).copied()
    }

    /// Outermost closed fold containing `line`: (start, end).
    pub fn closed_fold_at(&self, buf: &Buffer, line: usize) -> Option<(usize, usize)> {
        if buf.folds.is_empty() || !buf.is_markdown() {
            return None;
        }
        let doc = buf.markdown();
        // Walk folds backwards from `line`; the outermost containing fold is
        // the one with the smallest start. A fold of level L that does not
        // contain `line` rules out every earlier fold of level >= L.
        let mut best = None;
        let mut min_level = u8::MAX;
        for &s in buf.folds.range(..=line).rev() {
            let Some(lvl) = doc.heading_level(s) else { continue };
            if lvl >= min_level {
                continue;
            }
            match doc.section_ends.get(&s) {
                Some(&e) if line <= e && e > s => best = Some((s, e)),
                _ => min_level = lvl,
            }
            if min_level <= 1 {
                break;
            }
        }
        best
    }

    /// First line of the display line containing `line` (fold start or itself).
    pub fn visible_line(&self, buf: &Buffer, line: usize) -> usize {
        self.closed_fold_at(buf, line).map(|(s, _)| s).unwrap_or(line)
    }

    pub fn next_visible(&self, buf: &Buffer, line: usize) -> Option<usize> {
        let end = self.closed_fold_at(buf, line).map(|(_, e)| e).unwrap_or(line);
        if end + 1 < buf.line_count() { Some(self.visible_line(buf, end + 1)) } else { None }
    }

    pub fn prev_visible(&self, buf: &Buffer, line: usize) -> Option<usize> {
        let start = self.closed_fold_at(buf, line).map(|(s, _)| s).unwrap_or(line);
        if start == 0 { None } else { Some(self.visible_line(buf, start - 1)) }
    }

    pub fn open_folds_at(&mut self, line: usize) {
        while let Some((s, _)) = self.closed_fold_at(self.buf(), line) {
            self.buf_mut().folds.remove(&s);
        }
    }

    // --- Layout ------------------------------------------------------------------

    pub fn line_view(&self, win: &Window, line: usize, width: usize, reveal: (usize, usize)) -> LineView {
        let buf = &self.buffers[&win.buf];
        let mode = self.render_mode(win, line, reveal);
        if let Some((s, e)) = self.closed_fold_at(buf, line)
            && s == line
        {
            let mut rl = self.render_buf_line(buf, line, mode);
            let suffix = format!("  ⋯ {} lines", e - s);
            for c in suffix.chars() {
                let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as u8;
                rl.cells.push(Cell { ch: c, width: w, flags: FOLD_FLAG, src: None });
            }
            let scale = self.heading_scale(buf, line, mode, &rl);
            let mut rows = std::iter::once(0..rl.cells.len()).collect::<Vec<_>>();
            if self.options.wrap {
                rows = wrap(&rl.cells, Self::wrap_width(width, scale), self.options.linebreak);
                rows.truncate(1);
            }
            return LineView { line, rl, rows, fold_end: Some(e), raw: mode != RenderMode::Preview, scale };
        }
        let rl = self.render_buf_line(buf, line, mode);
        let scale = self.heading_scale(buf, line, mode, &rl);
        let rows = if self.options.wrap {
            wrap(&rl.cells, Self::wrap_width(width, scale), self.options.linebreak)
        } else {
            std::iter::once(0..rl.cells.len()).collect()
        };
        LineView { line, rl, rows, fold_end: None, raw: mode != RenderMode::Preview, scale }
    }

    /// Width in normal-size cells available to a (possibly scaled) row.
    fn wrap_width(width: usize, scale: Option<Scale>) -> usize {
        match scale {
            // Leave room for the last packed group rounding up.
            Some(sc) => ((width.saturating_sub(sc.s as usize)) as f32 / sc.factor()).floor().max(1.0) as usize,
            None => width,
        }
    }

    /// Size previewed headings with the kitty text-sizing protocol where the
    /// terminal supports it; otherwise they are styled by weight only.
    fn heading_scale(&self, buf: &Buffer, line: usize, mode: RenderMode, rl: &RenderedLine) -> Option<Scale> {
        if mode != RenderMode::Preview || !buf.is_markdown() || rl.cells.is_empty() {
            return None;
        }
        if !(self.text_sizing && self.options.headingscale) {
            return None;
        }
        Scale::for_heading(buf.markdown().heading_level(line)?)
    }

    /// Lines displayed in a window, from its top line, filling its height.
    pub fn layout_window(&self, id: WinId) -> Vec<LineView> {
        let win = &self.windows[&id];
        let buf = &self.buffers[&win.buf];
        let rect = self.text_rect(id);
        let (h, w) = (rect.h as usize, rect.w as usize);
        let reveal = self.reveal_range(win);
        let mut out = Vec::new();
        let mut rows = 0;
        let mut line = self.visible_line(buf, win.top.min(buf.line_count() - 1));
        while rows < h {
            let lv = self.line_view(win, line, w, reveal);
            rows += lv.height();
            out.push(lv);
            match self.next_visible(buf, line) {
                Some(n) => line = n,
                None => break,
            }
        }
        out
    }

    fn line_height(&self, win: &Window, line: usize, width: usize, reveal: (usize, usize)) -> usize {
        self.line_view(win, line, width, reveal).height()
    }

    /// Row within the line and display x of a char column, for a rendered line.
    pub fn locate_col(lv: &LineView, col: usize) -> (usize, usize) {
        let cells = &lv.rl.cells;
        let idx = cells
            .iter()
            .position(|c| c.src.is_some_and(|s| s >= col))
            .unwrap_or(cells.len());
        for (r, range) in lv.rows.iter().enumerate() {
            if idx < range.end || (r + 1 == lv.rows.len()) {
                let x: usize = cells[range.start..idx.max(range.start).min(range.end)].iter().map(|c| c.width as usize).sum();
                return (r, x);
            }
        }
        (0, 0)
    }

    /// Source column of the cell at display x in row `row` of `lv`.
    pub fn col_at(lv: &LineView, row: usize, x: usize) -> usize {
        let cells = &lv.rl.cells;
        let Some(range) = lv.rows.get(row) else {
            return cells.iter().rev().find_map(|c| c.src).map(|s| s + 1).unwrap_or(0);
        };
        let mut acc = 0;
        let mut last_src = None;
        for c in &cells[range.clone()] {
            if c.src.is_some() {
                last_src = c.src;
            }
            if acc + c.width as usize > x {
                return c.src.or(last_src).unwrap_or(0);
            }
            acc += c.width as usize;
        }
        // Past the end of the row: last char of the row (or line end).
        if row + 1 == lv.rows.len() {
            cells.iter().rev().find_map(|c| c.src).map(|s| s + 1).unwrap_or(0)
        } else {
            last_src.unwrap_or(0)
        }
    }

    // --- Scrolling -----------------------------------------------------------------

    pub fn scroll_to_cursor(&mut self) {
        let id = self.win_id();
        let rect = self.text_rect(id);
        let (h, width) = (rect.h as usize, rect.w as usize);
        if h == 0 || width == 0 {
            return;
        }
        let win = &self.windows[&id];
        let buf = &self.buffers[&win.buf];
        let cur = self.visible_line(buf, win.cursor.line);
        let reveal = self.reveal_range(win);
        let so = self.options.scrolloff.min((h - 1) / 2);
        let mut top = self.visible_line(buf, win.top.min(buf.line_count() - 1));

        let mut top_min = cur;
        for _ in 0..so {
            match self.prev_visible(buf, top_min) {
                Some(p) => top_min = p,
                None => break,
            }
        }
        if top > top_min {
            top = top_min;
        }
        let mut bottom = cur;
        for _ in 0..so {
            match self.next_visible(buf, bottom) {
                Some(n) => bottom = n,
                None => break,
            }
        }
        // Do lines top..=bottom fit? (Stops as soon as the height is exceeded.)
        let fits = |top: usize| {
            let mut rows = 0;
            let mut l = top;
            loop {
                rows += self.line_height(win, l, width, reveal);
                if rows > h {
                    return false;
                }
                if l >= bottom {
                    return true;
                }
                match self.next_visible(buf, l) {
                    Some(n) => l = n,
                    None => return true,
                }
            }
        };
        if top > cur {
            top = cur;
        } else if !fits(top) {
            // Walk back from the bottom line to find the highest top that fits.
            let mut rows = self.line_height(win, bottom, width, reveal);
            let mut t = bottom;
            while let Some(p) = self.prev_visible(buf, t) {
                let ph = self.line_height(win, p, width, reveal);
                if rows + ph > h {
                    break;
                }
                rows += ph;
                t = p;
            }
            // The cursor line itself must stay visible.
            top = t.min(cur);
        }

        // Horizontal scrolling when not wrapping.
        let mut leftcol = win.leftcol;
        if !self.options.wrap {
            let lv = self.line_view(win, win.cursor.line, usize::MAX / 2, reveal);
            let (_, x) = Self::locate_col(&lv, win.cursor.col);
            let siso = self.options.sidescrolloff.min(width / 2);
            if x < leftcol + siso {
                leftcol = x.saturating_sub(siso);
            } else if x + siso >= leftcol + width {
                leftcol = x + siso + 1 - width;
            }
        } else {
            leftcol = 0;
        }
        let w = self.windows.get_mut(&id).unwrap();
        w.top = top;
        w.leftcol = leftcol;
    }

    pub fn scroll_to_cursor_center_if_needed(&mut self) {
        let before = self.win().top;
        self.scroll_to_cursor();
        if self.win().top != before {
            self.scroll_cursor_to(ScrollPos::Center);
        }
    }

    /// Lines fully visible in the current window.
    pub fn visible_line_list(&self) -> Vec<usize> {
        let id = self.win_id();
        let h = self.text_rect(id).h as usize;
        let mut rows = 0;
        let mut out = Vec::new();
        for lv in self.layout_window(id) {
            rows += lv.height();
            if rows > h && !out.is_empty() {
                break;
            }
            out.push(lv.line);
        }
        out
    }

    pub fn scroll_cursor_to(&mut self, at: ScrollPos) {
        let id = self.win_id();
        let rect = self.text_rect(id);
        let (h, width) = (rect.h as usize, rect.w as usize);
        let win = &self.windows[&id];
        let buf = &self.buffers[&win.buf];
        let cur = self.visible_line(buf, win.cursor.line);
        let reveal = self.reveal_range(win);
        let budget = match at {
            ScrollPos::Top => 0,
            ScrollPos::Center => h.saturating_sub(self.line_height(win, cur, width, reveal)) / 2,
            ScrollPos::Bottom => h.saturating_sub(self.line_height(win, cur, width, reveal)),
        };
        let mut top = cur;
        let mut used = 0;
        while let Some(p) = self.prev_visible(buf, top) {
            let lh = self.line_height(win, p, width, reveal);
            if used + lh > budget {
                break;
            }
            used += lh;
            top = p;
        }
        self.win_mut().top = top;
    }

    /// Scroll the view by `n` lines (positive = down), keeping the cursor visible.
    pub fn scroll_lines(&mut self, n: isize) {
        let mut top = self.win().top;
        for _ in 0..n.unsigned_abs() {
            let next = if n > 0 { self.next_visible(self.buf(), top) } else { self.prev_visible(self.buf(), top) };
            match next {
                Some(t) => top = t,
                None => break,
            }
        }
        self.win_mut().top = top;
        let vis = self.visible_line_list();
        let so = self.options.scrolloff.min(vis.len().saturating_sub(1) / 2);
        let cur = self.cursor().line;
        if let (Some(&first), Some(&last)) = (vis.first(), vis.last()) {
            let lo = vis.get(if first == 0 { 0 } else { so }).copied().unwrap_or(first);
            let hi_idx = vis.len().saturating_sub(1 + if last + 1 >= self.buf().line_count() { 0 } else { so });
            let hi = vis.get(hi_idx).copied().unwrap_or(last);
            let target = if cur < lo {
                Some(lo)
            } else if cur > hi {
                Some(hi)
            } else {
                None
            };
            if let Some(l) = target {
                let col = self.col_for_want(l);
                self.win_mut().cursor = Pos::new(l, col);
            }
        }
    }

    /// Column on `line` honouring the window's desired column.
    pub fn col_for_want(&self, line: usize) -> usize {
        let want = self.win().want_col;
        let len = self.buf().line_len(line);
        let max = if self.mode.is_insert() { len } else { len.saturating_sub(1) };
        if want == usize::MAX { max } else { want.min(max) }
    }

    /// `gj`/`gk`: move by display rows within wrapped lines.
    pub fn display_move(&mut self, down: bool, count: usize) {
        let id = self.win_id();
        let width = self.text_rect(id).w as usize;
        if !self.options.wrap || width == 0 {
            let m = if down { super::normal::Motion::Down } else { super::normal::Motion::Up };
            self.do_motion(&m, Some(count));
            return;
        }
        let raw_all = (0, usize::MAX);
        let mut pos = self.cursor();
        let x = {
            let win = &self.windows[&id];
            let lv = self.line_view(win, pos.line, width, raw_all);
            Self::locate_col(&lv, pos.col).1
        };
        for _ in 0..count {
            let win = &self.windows[&id];
            let lv = self.line_view(win, pos.line, width, raw_all);
            let (row, _) = Self::locate_col(&lv, pos.col);
            if down {
                if row + 1 < lv.rows.len() {
                    pos.col = Self::col_at(&lv, row + 1, x);
                } else if let Some(n) = self.next_visible(self.buf(), pos.line) {
                    let nv = self.line_view(win, n, width, raw_all);
                    pos = Pos::new(n, Self::col_at(&nv, 0, x));
                }
            } else if row > 0 {
                pos.col = Self::col_at(&lv, row - 1, x);
            } else if let Some(p) = self.prev_visible(self.buf(), pos.line) {
                let pv = self.line_view(win, p, width, raw_all);
                pos = Pos::new(p, Self::col_at(&pv, pv.rows.len() - 1, x));
            }
        }
        self.set_cursor(pos);
    }

    /// Map a screen cell to (window, buffer position).
    pub fn screen_to_pos(&self, x: u16, y: u16) -> Option<(WinId, Pos)> {
        let rects = super::layout::rects(&self.tab().root, self.editor_area());
        let (id, _) = rects.into_iter().find(|(_, r)| r.contains(x, y))?;
        let tr = self.text_rect(id);
        if y >= tr.y + tr.h {
            return None;
        }
        let win = &self.windows[&id];
        let rel_y = (y - tr.y) as usize;
        let rel_x = (x.saturating_sub(tr.x)) as usize + win.leftcol;
        let mut row = 0;
        let views = self.layout_window(id);
        for lv in &views {
            if rel_y < row + lv.height() {
                let x = match lv.scale {
                    Some(sc) => (rel_x as f32 / sc.factor()) as usize,
                    None => rel_x,
                };
                let col = Self::col_at(lv, lv.row_at(rel_y - row), x);
                return Some((id, Pos::new(lv.line, col)));
            }
            row += lv.height();
        }
        views.last().map(|lv| (id, Pos::new(lv.line, 0)))
    }

}

#[derive(Clone, Copy)]
pub enum ScrollPos {
    Top,
    Center,
    Bottom,
}
