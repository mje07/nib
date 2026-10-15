//! Line rendering: source line → display cells.

use std::ops::Range;

use pulldown_cmark::Alignment;
use unicode_width::UnicodeWidthChar;

use super::flags::*;
use super::{LineKind, MdDoc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub width: u8,
    pub flags: u32,
    /// Char column in the source line this cell came from.
    pub src: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct RenderedLine {
    pub cells: Vec<Cell>,
    /// Character (and flags) repeated to fill the rest of the last row.
    pub fill: Option<(char, u32)>,
}

impl RenderedLine {
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.cells.iter().filter(|c| c.width > 0 || c.ch != '\0').map(|c| c.ch).collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderMode {
    /// Live preview: markup hidden, replacements applied.
    Preview,
    /// Source with markdown highlighting (cursor line / source mode).
    Raw,
    /// No highlighting (non-markdown files).
    Plain,
}

fn push_char(cells: &mut Vec<Cell>, c: char, flags: u32, src: Option<usize>, tabstop: usize) {
    if c == '\t' {
        let col: usize = cells.iter().map(|c| c.width as usize).sum();
        let n = tabstop - (col % tabstop.max(1));
        for i in 0..n {
            cells.push(Cell { ch: ' ', width: 1, flags, src: if i == 0 { src } else { None } });
        }
    } else if (c as u32) < 0x20 || c == '\x7f' {
        let shown = if c == '\x7f' { '?' } else { char::from_u32(c as u32 + 0x40).unwrap_or('?') };
        cells.push(Cell { ch: '^', width: 1, flags, src });
        cells.push(Cell { ch: shown, width: 1, flags, src: None });
    } else {
        let w = c.width().unwrap_or(0) as u8;
        cells.push(Cell { ch: c, width: w, flags, src });
    }
}

/// Render source line `line` (text `s`).
pub fn render_line(doc: Option<&MdDoc>, s: &str, line: usize, mode: RenderMode, tabstop: usize) -> RenderedLine {
    let doc = match (doc, mode) {
        (Some(d), RenderMode::Preview | RenderMode::Raw) if line < d.line_count() => d,
        _ => return plain(s, tabstop),
    };
    if mode == RenderMode::Raw {
        return raw(doc, line, tabstop);
    }
    let lr = doc.line_range(line);
    match &doc.kinds[line] {
        LineKind::FenceOpen(lang) => {
            let mut cells = Vec::new();
            let label = if lang.is_empty() { String::new() } else { format!(" {lang} ") };
            for c in label.chars() {
                push_char(&mut cells, c, CODEFENCE, Some(0), tabstop);
            }
            RenderedLine { cells, fill: Some((' ', CODEBLOCK)) }
        }
        LineKind::FenceClose => RenderedLine { cells: vec![], fill: Some((' ', CODEBLOCK)) },
        LineKind::CodeBody => {
            let mut r = raw(doc, line, tabstop);
            for c in &mut r.cells {
                c.flags = CODEBLOCK;
            }
            r.fill = Some((' ', CODEBLOCK));
            r
        }
        LineKind::Hr => RenderedLine { cells: vec![], fill: Some(('─', HR)) },
        LineKind::FrontMatter => raw(doc, line, tabstop),
        LineKind::TableDelim(t) => table_delim(doc, *t),
        LineKind::TableHeader(t) | LineKind::TableBody(t) => table_row(doc, *t, line, tabstop),
        _ => {
            let mut cells = Vec::new();
            preview_range(doc, lr, line, &mut cells, tabstop);
            RenderedLine { cells, fill: None }
        }
    }
}

fn plain(s: &str, tabstop: usize) -> RenderedLine {
    let mut cells = Vec::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        push_char(&mut cells, c, 0, Some(i), tabstop);
    }
    RenderedLine { cells, fill: None }
}

fn raw(doc: &MdDoc, line: usize, tabstop: usize) -> RenderedLine {
    let lr = doc.line_range(line);
    let mut cells = Vec::with_capacity(lr.len());
    for (i, (b, c)) in doc.text[lr.clone()].char_indices().enumerate() {
        push_char(&mut cells, c, doc.flags[lr.start + b], Some(i), tabstop);
    }
    RenderedLine { cells, fill: None }
}

/// Preview-render a byte range of the doc into `cells`. `src` columns are
/// relative to the start of `line`.
fn preview_range(doc: &MdDoc, r: Range<usize>, line: usize, cells: &mut Vec<Cell>, tabstop: usize) {
    let text = &doc.text;
    let mut col = text[doc.line_starts[line]..r.start].chars().count();
    let mut b = r.start;
    while b < r.end {
        let c = text[b..].chars().next().unwrap();
        if let Some(rep) = doc.replace.get(&b) {
            for rc in rep.text.chars() {
                push_char(cells, rc, rep.flags | (doc.flags[b] & !(MARKUP | LISTMARK)), Some(col), tabstop);
            }
            if rep.len > 0 {
                col += text[b..b + rep.len].chars().count();
                b += rep.len;
                continue;
            }
        }
        let f = doc.flags[b];
        if f & MARKUP == 0 {
            push_char(cells, c, f, Some(col), tabstop);
        }
        col += 1;
        b += c.len_utf8();
    }
}

/// Rendered cells of one table cell, trimmed (used for widths and rows).
pub fn cell_cells(doc: &MdDoc, r: Range<usize>) -> Vec<Cell> {
    let line = doc.line_of(r.start);
    let mut cells = Vec::new();
    preview_range(doc, r, line, &mut cells, 4);
    while cells.first().is_some_and(|c| c.ch == ' ') {
        cells.remove(0);
    }
    while cells.last().is_some_and(|c| c.ch == ' ') {
        cells.pop();
    }
    cells
}

fn border(cells: &mut Vec<Cell>, s: &str) {
    for c in s.chars() {
        cells.push(Cell { ch: c, width: 1, flags: TABLEBORDER, src: None });
    }
}

fn table_delim(doc: &MdDoc, t: usize) -> RenderedLine {
    let info = &doc.tables[t];
    let mut cells = Vec::new();
    border(&mut cells, "├");
    for (i, w) in info.widths.iter().enumerate() {
        if i > 0 {
            border(&mut cells, "┼");
        }
        border(&mut cells, &"─".repeat(w + 2));
    }
    border(&mut cells, "┤");
    if let Some(c) = cells.first_mut() {
        c.src = Some(0);
    }
    RenderedLine { cells, fill: None }
}

fn table_row(doc: &MdDoc, t: usize, line: usize, tabstop: usize) -> RenderedLine {
    let info = &doc.tables[t];
    let Some(row) = info.cells.get(&line) else {
        return raw(doc, line, tabstop);
    };
    let mut cells = Vec::new();
    for (i, w) in info.widths.iter().enumerate() {
        border(&mut cells, if i == 0 { "│ " } else { " │ " });
        let content = row.get(i).map(|r| cell_cells(doc, r.clone())).unwrap_or_default();
        let cw: usize = content.iter().map(|c| c.width as usize).sum();
        let pad = w.saturating_sub(cw);
        let (left, right) = match info.aligns.get(i) {
            Some(Alignment::Right) => (pad, 0),
            Some(Alignment::Center) => (pad / 2, pad - pad / 2),
            _ => (0, pad),
        };
        let pad_flags = if matches!(doc.kinds[line], LineKind::TableHeader(_)) { TABLEHDR } else { 0 };
        for _ in 0..left {
            cells.push(Cell { ch: ' ', width: 1, flags: pad_flags, src: None });
        }
        cells.extend(content);
        for _ in 0..right {
            cells.push(Cell { ch: ' ', width: 1, flags: pad_flags, src: None });
        }
    }
    border(&mut cells, " │");
    if let Some(c) = cells.first_mut() {
        c.src = Some(0);
    }
    RenderedLine { cells, fill: None }
}

/// Split cells into rows of at most `width` columns. With `linebreak`, rows
/// break after whitespace when possible. Returns cell index ranges.
pub fn wrap(cells: &[Cell], width: usize, linebreak: bool) -> Vec<Range<usize>> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        let mut w = 0;
        let mut end = start;
        while end < cells.len() && w + cells[end].width as usize <= width {
            w += cells[end].width as usize;
            end += 1;
        }
        if end == start {
            end = start + 1; // a char wider than the window
        }
        if end < cells.len() && linebreak {
            // Break after the last space in the row, if any.
            if let Some(sp) = cells[start..end].iter().rposition(|c| c.ch == ' ')
                && sp > 0
            {
                end = start + sp + 1;
            }
        }
        // Zero-width chars stick to the preceding cell.
        while end < cells.len() && cells[end].width == 0 {
            end += 1;
        }
        rows.push(start..end);
        start = end;
    }
    if rows.is_empty() {
        rows.push(0..0);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prev(src: &str, line: usize) -> String {
        let doc = MdDoc::parse(src);
        let s = doc.text[doc.line_range(line)].to_string();
        render_line(Some(&doc), &s, line, RenderMode::Preview, 4).text()
    }

    #[test]
    fn headings_and_emphasis() {
        assert_eq!(prev("# Title", 0), "Title");
        assert_eq!(prev("### Sub ###", 0), "Sub");
        assert_eq!(prev("some **bold** and *it* and ~~st~~", 0), "some bold and it and st");
        assert_eq!(prev("`code` here", 0), "code here");
        assert_eq!(prev("a [link](http://x.y) b", 0), "a link b");
        assert_eq!(prev("![alt](img.png)", 0), "🖼 alt");
        assert_eq!(prev("x ==hi== y", 0), "x hi y");
        assert_eq!(prev(r"a \*b\* c", 0), "a *b* c");
    }

    #[test]
    fn lists_tasks_quotes() {
        assert_eq!(prev("- item", 0), "• item");
        assert_eq!(prev("- a\n  - b", 1), "  • b");
        assert_eq!(prev("- [ ] todo", 0), "☐ todo");
        assert_eq!(prev("- [x] done", 0), "☑ done");
        assert_eq!(prev("1. one", 0), "1. one");
        assert_eq!(prev("> quoted", 0), "▎ quoted");
        assert_eq!(prev("> > nested", 0), "▎ ▎ nested");
    }

    #[test]
    fn tags() {
        let doc = MdDoc::parse("see #todo and #2024 x#no (#yes)");
        let tagged: String = doc.text.char_indices().filter(|(b, _)| doc.flags[*b] & TAG != 0).map(|(_, c)| c).collect();
        assert_eq!(tagged, "#todo#yes");
        assert_eq!(MdDoc::parse("#tag").kinds[0], LineKind::Normal, "#tag is not a heading");
    }

    #[test]
    fn tables() {
        let src = "| a | bb |\n|---|:-:|\n| ccc | d |";
        assert_eq!(prev(src, 0), "│ a   │ bb  │");
        assert_eq!(prev(src, 1), "├─────┼─────┤");
        assert_eq!(prev(src, 2), "│ ccc │  d  │");
    }

    #[test]
    fn code_blocks_and_hr() {
        let src = "```rust\nlet x = *y*;\n```\n\n---";
        let doc = MdDoc::parse(src);
        assert_eq!(doc.kinds[0], LineKind::FenceOpen("rust".into()));
        assert_eq!(doc.kinds[1], LineKind::CodeBody);
        assert_eq!(doc.kinds[2], LineKind::FenceClose);
        assert_eq!(doc.group(1), Some((0, 2)));
        assert_eq!(prev(src, 1), "let x = *y*;");
        assert_eq!(doc.kinds[4], LineKind::Hr);
    }

    #[test]
    fn wrapping() {
        let r = plain("hello world foo", 4);
        let rows = wrap(&r.cells, 8, true);
        assert_eq!(rows, vec![0..6, 6..12, 12..15]);
    }
}
