//! Drawing the editor state into a ratatui buffer.

use std::collections::HashMap;

use ratatui::buffer::Buffer as TBuf;
use ratatui::layout::Rect as TRect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::editor::view::{FOLD_FLAG, Scale};
use crate::editor::{Editor, Mode, MsgKind, VKind};
use crate::key::keys_to_notation;
use crate::markdown::flags::*;
use crate::theme::Theme;
use crate::types::{Pos, Rect, WinId};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

pub struct DrawResult {
    pub cursor: Option<(u16, u16)>,
    pub shape: CursorShape,
    /// Scaled heading text to draw with the kitty text-sizing protocol after
    /// the cell buffer has been flushed (ratatui only sees blank cells there).
    pub overlays: Vec<TextOverlay>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextOverlay {
    pub x: u16,
    pub y: u16,
    pub scale: Scale,
    /// Characters with their normal-size cell widths.
    pub chars: Vec<(char, u8)>,
    pub style: Style,
    /// Screen columns covered.
    pub width: u16,
}

impl TextOverlay {
    fn intersects(&self, r: Rect) -> bool {
        let h = self.scale.s as u16;
        self.x < r.x + r.w && self.x + self.width > r.x && self.y < r.y + r.h && self.y + h > r.y
    }

    /// Escape sequences drawing this overlay at its position.
    pub fn escape(&self) -> String {
        let Scale { s, n, d } = self.scale;
        let mut out = format!("\x1b[{};{}H{}", self.y + 1, self.x + 1, sgr(self.style.fg, self.style.bg, self.style.add_modifier));
        let clean = |c: char| if (c as u32) < 0x20 || c == '\x7f' { ' ' } else { c };
        if n == 0 || d == 0 {
            let text: String = self.chars.iter().map(|(c, _)| clean(*c)).collect();
            out.push_str(&format!("\x1b]66;s={s};{text}\x07"));
        } else {
            // Pack characters into blocks so fractional sizes stay compact:
            // a block of w cells holds characters totalling w*d/n normal cells.
            let (n, d) = (n as usize, d as usize);
            let mut group = String::new();
            let mut gw = 0usize;
            let flush = |group: &mut String, gw: &mut usize, out: &mut String| {
                if *gw > 0 {
                    let w = (*gw * n).div_ceil(d);
                    out.push_str(&format!("\x1b]66;s={s}:n={n}:d={d}:w={w}:v=2;{group}\x07"));
                    group.clear();
                    *gw = 0;
                }
            };
            for (c, cw) in &self.chars {
                let cw = *cw as usize;
                if (gw + cw) * n > 7 * d {
                    flush(&mut group, &mut gw, &mut out);
                }
                group.push(clean(*c));
                gw += cw;
                if (gw * n).is_multiple_of(d) {
                    flush(&mut group, &mut gw, &mut out);
                }
            }
            flush(&mut group, &mut gw, &mut out);
        }
        out.push_str("\x1b[0m");
        out
    }
}

/// SGR escape for a style (also used by `--dump-render --ansi`).
pub fn sgr(fg: Option<ratatui::style::Color>, bg: Option<ratatui::style::Color>, m: Modifier) -> String {
    use ratatui::style::Color;
    let mut codes = vec!["0".to_string()];
    for (bit, code) in [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::ITALIC, "3"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::REVERSED, "7"),
        (Modifier::CROSSED_OUT, "9"),
    ] {
        if m.contains(bit) {
            codes.push(code.into());
        }
    }
    let color = |col: Color, base: u8| -> Option<String> {
        Some(match col {
            Color::Reset => return None,
            Color::Black => format!("{}", base),
            Color::Red => format!("{}", base + 1),
            Color::Green => format!("{}", base + 2),
            Color::Yellow => format!("{}", base + 3),
            Color::Blue => format!("{}", base + 4),
            Color::Magenta => format!("{}", base + 5),
            Color::Cyan => format!("{}", base + 6),
            Color::Gray => format!("{}", base + 7),
            Color::DarkGray => format!("{}", base + 60),
            Color::LightRed => format!("{}", base + 61),
            Color::LightGreen => format!("{}", base + 62),
            Color::LightYellow => format!("{}", base + 63),
            Color::LightBlue => format!("{}", base + 64),
            Color::LightMagenta => format!("{}", base + 65),
            Color::LightCyan => format!("{}", base + 66),
            Color::White => format!("{}", base + 67),
            Color::Indexed(i) => format!("{};5;{}", base + 8, i),
            Color::Rgb(r, g, b) => format!("{};2;{};{};{}", base + 8, r, g, b),
        })
    };
    if let Some(s) = fg.and_then(|c| color(c, 30)) {
        codes.push(s);
    }
    if let Some(s) = bg.and_then(|c| color(c, 40)) {
        codes.push(s);
    }
    format!("\x1b[{}m", codes.join(";"))
}

pub fn md_style(theme: &Theme, f: u32) -> Style {
    let mut s = Style::default();
    let h = heading_level(f);
    if h > 0 {
        s = s.patch(theme.get(&format!("h{h}")));
    }
    let groups: &[(u32, &str)] = &[
        (CODEBLOCK, "codeblock"),
        (CODEFENCE, "codefence"),
        (FRONTMATTER, "frontmatter"),
        (QUOTE, "quote"),
        (QUOTEBAR, "quotebar"),
        (BOLD, "bold"),
        (ITALIC, "italic"),
        (STRIKE, "strike"),
        (HIGHLIGHT, "highlight"),
        (CODE, "code"),
        (LINK, "link"),
        (IMAGE, "image"),
        (MATH, "math"),
        (FOOTNOTE, "footnote"),
        (HTML, "html"),
        (LISTMARK, "listmarker"),
        (TASKBOX, "taskbox"),
        (TASKDONE, "taskdone"),
        (TABLEHDR, "tableheader"),
        (TABLEBORDER, "tableborder"),
        (HR, "hr"),
        (TAG, "tag"),
        (FOLD_FLAG, "fold"),
        (MARKUP, "markup"),
    ];
    for (bit, name) in groups {
        if f & bit != 0 {
            s = s.patch(theme.get(name));
        }
    }
    // Markup characters shown on raw lines should not inherit link underline etc.
    if f & MARKUP != 0 {
        s = s.remove_modifier(Modifier::UNDERLINED | Modifier::CROSSED_OUT);
    }
    s
}

fn put(buf: &mut TBuf, x: u16, y: u16, s: &str, style: Style, max_w: u16) -> u16 {
    if max_w == 0 {
        return 0;
    }
    let (nx, _) = buf.set_stringn(x, y, s, max_w as usize, style);
    nx - x
}

fn fill(buf: &mut TBuf, r: Rect, style: Style) {
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(" ");
                c.set_style(style);
            }
        }
    }
}

struct Sel {
    kind: VKind,
    a: Pos,
    b: Pos,
    c1: usize,
    c2: usize,
}

impl Sel {
    fn contains(&self, line: usize, col: usize) -> bool {
        if line < self.a.line || line > self.b.line {
            return false;
        }
        match self.kind {
            VKind::Line => true,
            VKind::Block => col >= self.c1 && col <= self.c2,
            VKind::Char => {
                (line > self.a.line || col >= self.a.col) && (line < self.b.line || col <= self.b.col)
            }
        }
    }
}

pub fn draw(ed: &Editor, buf: &mut TBuf, area: TRect) -> DrawResult {
    let (w, h) = (area.width, area.height);
    let theme = &ed.theme;
    fill(buf, Rect::new(0, 0, w, h), theme.get("normal"));
    let mut cursor = None;

    if ed.tabs.len() > 1 {
        draw_tabline(ed, buf, w);
    }

    let rects = crate::editor::layout::rects(&ed.tab().root, ed.editor_area());
    let ea = ed.editor_area();
    let mut overlays = Vec::new();
    for (id, r) in &rects {
        if let Some(c) = draw_window(ed, buf, *id, *r, &mut overlays) {
            cursor = Some(c);
        }
        if r.x + r.w < ea.x + ea.w {
            for y in r.y..r.y + r.h {
                put(buf, r.x + r.w, y, "│", theme.get("vertsplit"), 1);
            }
        }
    }

    // Command line / messages.
    let row = h.saturating_sub(1);
    let mut shape = match ed.mode {
        Mode::Insert => CursorShape::Bar,
        Mode::Replace => CursorShape::Underline,
        _ => CursorShape::Block,
    };
    if ed.insert.ctrl_o {
        shape = CursorShape::Block;
    }
    if ed.mode == Mode::Cmdline {
        let c = &ed.cmdline;
        let text = format!("{}{}", c.kind, c.text);
        let before: String = std::iter::once(c.kind).chain(c.text.chars().take(c.cursor)).collect();
        let cx = before.width() as u16;
        let skip = if cx >= w { cx - w + 1 } else { 0 };
        let shown: String = skip_width(&text, skip as usize);
        put(buf, 0, row, &shown, theme.get("normal"), w);
        cursor = Some((cx - skip, row));
        shape = CursorShape::Bar;
    } else if ed.hit_enter && ed.messages.len() > 1 {
        let n = (ed.messages.len() + 1).min(h as usize);
        let start = h as usize - n;
        fill(buf, Rect::new(0, start as u16, w, n as u16), theme.get("normal"));
        overlays.retain(|o| !o.intersects(Rect::new(0, start as u16, w, n as u16)));
        for (i, (m, k)) in ed.messages.iter().skip(ed.messages.len() + 1 - n).enumerate() {
            let st = if *k == MsgKind::Error { theme.get("error") } else { theme.get("normal") };
            put(buf, 0, (start + i) as u16, m, st, w);
        }
        put(buf, 0, row, "Press ENTER or type command to continue", theme.get("moremsg"), w);
        cursor = Some(("Press ENTER or type command to continue".len() as u16).min(w.saturating_sub(1))).map(|x| (x, row));
    } else if let Some((m, k)) = ed.messages.first() {
        let st = if *k == MsgKind::Error { theme.get("error") } else { theme.get("normal") };
        put(buf, 0, row, m, st, w);
    } else if ed.options.showmode {
        let label = match ed.mode {
            Mode::Insert if ed.insert.ctrl_o => "",
            Mode::Insert => "-- INSERT --",
            Mode::Replace => "-- REPLACE --",
            Mode::Visual(VKind::Char) => "-- VISUAL --",
            Mode::Visual(VKind::Line) => "-- VISUAL LINE --",
            Mode::Visual(VKind::Block) => "-- VISUAL BLOCK --",
            _ if ed.insert.ctrl_o => "-- (insert) --",
            _ => "",
        };
        let n = put(buf, 0, row, label, theme.get("moremsg"), w);
        if let Some((r, _)) = &ed.recording {
            put(buf, n + if n > 0 { 1 } else { 0 }, row, &format!("recording @{r}"), theme.get("moremsg"), w.saturating_sub(n));
        }
    }
    if ed.options.showcmd && ed.mode != Mode::Cmdline && w > 12 {
        let pend = keys_to_notation(&ed.pending_keys());
        let pend: String = pend.chars().rev().take(10).collect::<Vec<_>>().into_iter().rev().collect();
        put(buf, w - 12, row, &pend, theme.get("normal"), 11);
    }

    // Insert-mode completion popup.
    if let (Some(comp), Some((cx, cy))) = (&ed.insert.completion, cursor)
        && ed.mode.is_insert()
    {
        let max = 10usize;
        let n = comp.items.len().min(max);
        let sel = comp.idx.unwrap_or(0);
        let first = if sel >= max { sel + 1 - max } else { 0 };
        let width = comp.items.iter().map(|s| s.width()).max().unwrap_or(0).min(40) + 2;
        let below = cy as usize + 1 + n < h as usize;
        let top = if below { cy + 1 } else { cy.saturating_sub(n as u16) };
        let start_x = cx.saturating_sub(comp.prefix.width() as u16).min(w.saturating_sub(width as u16));
        overlays.retain(|o| !o.intersects(Rect::new(start_x, top, width as u16, n as u16)));
        for (i, item) in comp.items.iter().skip(first).take(n).enumerate() {
            let st = if comp.idx == Some(first + i) { theme.get("pmenusel") } else { theme.get("pmenu") };
            let text = format!(" {:<width$}", item, width = width - 1);
            put(buf, start_x, top + i as u16, &text, st, width as u16);
        }
    }

    DrawResult { cursor, shape, overlays }
}

fn skip_width(s: &str, cols: usize) -> String {
    let mut acc = 0;
    let mut out = String::new();
    for c in s.chars() {
        if acc >= cols {
            out.push(c);
        }
        acc += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
    }
    out
}

fn draw_tabline(ed: &Editor, buf: &mut TBuf, w: u16) {
    let theme = &ed.theme;
    fill(buf, Rect::new(0, 0, w, 1), theme.get("tablinefill"));
    let mut x = 0;
    for (i, t) in ed.tabs.iter().enumerate() {
        let win = &ed.windows[&t.active];
        let b = &ed.buffers[&win.buf];
        let name = b.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or("[No Name]".into());
        let modified = crate::editor::layout::leaves(&t.root).iter().any(|w| ed.buffers[&ed.windows[w].buf].is_modified());
        let label = format!(" {} {}{} ", i + 1, name, if modified { " +" } else { "" });
        let st = if i == ed.tab { theme.get("tablinesel") } else { theme.get("tabline") };
        x += put(buf, x, 0, &label, st, w.saturating_sub(x));
        if x >= w {
            break;
        }
    }
}

fn draw_window(ed: &Editor, buf: &mut TBuf, id: WinId, r: Rect, overlays: &mut Vec<TextOverlay>) -> Option<(u16, u16)> {
    let theme = &ed.theme;
    let win = &ed.windows[&id];
    let b = &ed.buffers[&win.buf];
    let tr = ed.text_rect(id);
    let g = ed.gutter_width(win.buf) as u16;
    let active = id == ed.win_id();
    let views = ed.layout_window(id);
    let hl = ed.highlight_regex();
    let inc = if active && ed.mode == Mode::Cmdline { ed.cmdline.inc_match } else { None };
    let sel = match ed.mode {
        Mode::Visual(kind) if active => {
            let (a, e) = if win.vstart <= win.cursor { (win.vstart, win.cursor) } else { (win.cursor, win.vstart) };
            let c1 = win.vstart.col.min(win.want_col);
            let c2 = if win.want_col == usize::MAX { usize::MAX } else { win.vstart.col.max(win.want_col) };
            Some(Sel { kind, a, b: e, c1, c2 })
        }
        _ => None,
    };
    let mut cache: HashMap<u32, Style> = HashMap::new();
    let cursor_line = win.cursor.line;
    let cul = ed.options.cursorline && active;
    let mut y = tr.y;
    let mut cursor = None;
    for lv in &views {
        let line_text = if hl.is_some() { b.line(lv.line) } else { String::new() };
        let matches = hl.as_ref().map(|re| ed.line_matches(re, &line_text)).unwrap_or_default();
        let is_cursor_line = lv.line == cursor_line || lv.fold_end.is_some_and(|e| cursor_line >= lv.line && cursor_line <= e);
        let base = if cul && is_cursor_line { theme.get("cursorline") } else { Style::default() };
        if active && is_cursor_line {
            let (row, x) = Editor::locate_col(lv, win.cursor.col);
            let x = lv.scale.map(|s| (x as f32 * s.factor()) as usize).unwrap_or(x);
            let ry = y as usize + row * lv.row_h();
            let cx = (x as isize - win.leftcol as isize).max(0) as u16;
            if ry < (tr.y + tr.h) as usize && ed.mode != Mode::Cmdline {
                cursor = Some((tr.x + cx.min(tr.w.saturating_sub(1)), ry as u16));
            }
        }
        for (ri, row) in lv.rows.iter().enumerate() {
            if y >= tr.y + tr.h {
                break;
            }
            if cul && is_cursor_line {
                fill(buf, Rect::new(tr.x, y, tr.w, 1), base);
            }
            if g > 0 {
                let num = if ri > 0 {
                    String::new()
                } else if ed.options.relativenumber && lv.line != cursor_line {
                    let d = (lv.line as isize - cursor_line as isize).unsigned_abs();
                    format!("{d:>width$} ", width = g as usize - 1)
                } else if ed.options.relativenumber && !ed.options.number {
                    format!("{:>width$} ", 0, width = g as usize - 1)
                } else if ed.options.relativenumber {
                    format!("{:<width$} ", lv.line + 1, width = g as usize - 1)
                } else {
                    format!("{:>width$} ", lv.line + 1, width = g as usize - 1)
                };
                let st = if is_cursor_line { theme.get("cursorlinenr") } else { theme.get("linenr") };
                put(buf, r.x, y, &num, st, g);
            }
            if let Some(sc) = lv.scale
                && y + sc.s as u16 <= tr.y + tr.h
            {
                let cells = &lv.rl.cells[row.clone()];
                let chars: Vec<(char, u8)> = cells.iter().filter(|c| c.width > 0).map(|c| (c.ch, c.width)).collect();
                let flags = cells.iter().find(|c| c.flags & FOLD_FLAG == 0).map(|c| c.flags).unwrap_or(0);
                let style = theme.get("normal").patch(base).patch(*cache.entry(flags).or_insert_with(|| md_style(theme, flags)));
                let base_w: usize = chars.iter().map(|(_, w)| *w as usize).sum();
                let width = ((base_w as f32 * sc.factor()).ceil() as u16 + sc.s as u16).min(tr.w);
                overlays.push(TextOverlay { x: tr.x, y, scale: sc, chars, style, width });
                y += sc.s as u16;
                continue;
            }
            let mut x = 0usize;
            let mut last_src = None;
            let cells = &lv.rl.cells[row.clone()];
            let mut skip = if ed.options.wrap { 0 } else { win.leftcol };
            for c in cells {
                if c.src.is_some() {
                    last_src = c.src;
                }
                let cw = c.width as usize;
                if skip > 0 {
                    skip = skip.saturating_sub(cw.max(1));
                    continue;
                }
                if x + cw > tr.w as usize {
                    break;
                }
                let mut st = *cache.entry(c.flags).or_insert_with(|| md_style(theme, c.flags));
                st = base.patch(st);
                let src = c.src.or(last_src);
                if let Some(col) = src {
                    if matches.iter().any(|(s, e)| col >= *s && col < *e) {
                        st = st.patch(theme.get("search"));
                    }
                    if let Some((s, e)) = inc
                        && Pos::new(lv.line, col) >= s
                        && Pos::new(lv.line, col) < e
                    {
                        st = st.patch(theme.get("incsearch"));
                    }
                    if let Some(sel) = &sel
                        && sel.contains(lv.line, col)
                    {
                        st = st.patch(theme.get("visual"));
                    }
                }
                if cw == 0 {
                    // Combining mark: append to the previous cell.
                    if x > 0
                        && let Some(cell) = buf.cell_mut((tr.x + x as u16 - 1, y))
                    {
                        let sym = format!("{}{}", cell.symbol(), c.ch);
                        cell.set_symbol(&sym);
                    }
                    continue;
                }
                let s = c.ch.to_string();
                put(buf, tr.x + x as u16, y, &s, st, cw as u16);
                x += cw;
            }
            let is_last_row = ri + 1 == lv.rows.len();
            // Selection covering the newline shows one highlighted cell.
            if is_last_row
                && let Some(sel) = &sel
                && sel.kind != VKind::Block
                && sel.contains(lv.line, b.line_len(lv.line))
                && lv.raw
                && (x as u16) < tr.w
            {
                put(buf, tr.x + x as u16, y, " ", base.patch(theme.get("visual")), 1);
                x += 1;
            }
            if is_last_row
                && let Some((ch, flags)) = lv.rl.fill
            {
                let st = base.patch(md_style(theme, flags));
                while (x as u16) < tr.w {
                    put(buf, tr.x + x as u16, y, &ch.to_string(), st, 1);
                    x += 1;
                }
            }
            y += 1;
        }
    }
    while y < tr.y + tr.h {
        put(buf, r.x, y, "~", theme.get("nontext"), r.w);
        y += 1;
    }
    draw_statusline(ed, buf, id, r, active);
    cursor
}

fn draw_statusline(ed: &Editor, buf: &mut TBuf, id: WinId, r: Rect, active: bool) {
    let theme = &ed.theme;
    let win = &ed.windows[&id];
    let b = &ed.buffers[&win.buf];
    let y = r.y + r.h.saturating_sub(1);
    let st = if active { theme.get("statusline") } else { theme.get("statuslinenc") };
    fill(buf, Rect::new(r.x, y, r.w, 1), st);
    let mut x = r.x;
    if active {
        let (label, group) = match ed.mode {
            Mode::Normal if ed.insert.ctrl_o => (" (INSERT) ", "mode_insert"),
            Mode::Normal => (" NORMAL ", "mode_normal"),
            Mode::Insert => (" INSERT ", "mode_insert"),
            Mode::Replace => (" REPLACE ", "mode_replace"),
            Mode::Visual(VKind::Char) => (" VISUAL ", "mode_visual"),
            Mode::Visual(VKind::Line) => (" V-LINE ", "mode_visual"),
            Mode::Visual(VKind::Block) => (" V-BLOCK ", "mode_visual"),
            Mode::Cmdline => (" COMMAND ", "mode_command"),
        };
        x += put(buf, x, y, label, theme.get(group), r.w);
    }
    let name = b.display_name();
    let left = format!(" {}{} ", name, if b.is_modified() { " [+]" } else { "" });
    x += put(buf, x, y, &left, st.add_modifier(Modifier::BOLD), r.w.saturating_sub(x - r.x));
    let lines = b.line_count();
    let pct = if win.top == 0 && ed.layout_window(id).last().is_some_and(|l| l.line + 1 >= lines) {
        "All".to_string()
    } else if win.top == 0 {
        "Top".to_string()
    } else if ed.layout_window(id).last().is_some_and(|l| l.line + 1 >= lines) {
        "Bot".to_string()
    } else {
        format!("{}%", (win.cursor.line + 1) * 100 / lines.max(1))
    };
    let mode = if b.is_markdown() {
        if ed.preview_on(win) { "markdown ◆ preview" } else { "markdown ◇ source" }
    } else {
        b.filetype.name()
    };
    let right = format!(" {}  {}:{}  {:>4} ", mode, win.cursor.line + 1, win.cursor.col + 1, pct);
    let rw = right.width() as u16;
    if r.x + r.w > rw && r.x + r.w - rw > x {
        put(buf, r.x + r.w - rw, y, &right, st, rw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlay(scale: Scale, text: &str) -> TextOverlay {
        TextOverlay {
            x: 2,
            y: 4,
            scale,
            chars: text.chars().map(|c| (c, 1)).collect(),
            style: Style::default(),
            width: 0,
        }
    }

    #[test]
    fn overlay_escapes() {
        let e = overlay(Scale { s: 2, n: 0, d: 0 }, "Hi").escape();
        assert!(e.starts_with("\x1b[5;3H"));
        assert!(e.contains("\x1b]66;s=2;Hi\x07"));
        // 1.5x: four normal cells pack into a block 3 wide.
        let e = overlay(Scale { s: 2, n: 3, d: 4 }, "abcdef").escape();
        assert!(e.contains("\x1b]66;s=2:n=3:d=4:w=3:v=2;abcd\x07"), "{e:?}");
        assert!(e.contains("\x1b]66;s=2:n=3:d=4:w=2:v=2;ef\x07"), "{e:?}");
    }
}
