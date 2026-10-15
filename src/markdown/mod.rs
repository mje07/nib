//! Markdown analysis for live preview.
//!
//! The whole buffer is parsed with pulldown-cmark into *per-byte* annotations:
//! style flags, a "markup" bit for syntax characters that preview hides, and
//! replacements (e.g. `-` → `•`, `[ ]` → `☐`). Rendering a line is then a
//! cheap walk over its bytes (see `render.rs`). Every source line maps to
//! exactly one display line, so scrolling and line numbers stay vim-like.

pub mod editing;
pub mod render;

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

pub mod flags {
    pub const BOLD: u32 = 1;
    pub const ITALIC: u32 = 1 << 1;
    pub const STRIKE: u32 = 1 << 2;
    pub const CODE: u32 = 1 << 3;
    pub const LINK: u32 = 1 << 4;
    pub const IMAGE: u32 = 1 << 5;
    pub const QUOTE: u32 = 1 << 6;
    pub const LISTMARK: u32 = 1 << 7;
    pub const TASKBOX: u32 = 1 << 8;
    pub const TASKDONE: u32 = 1 << 9;
    pub const CODEBLOCK: u32 = 1 << 10;
    pub const CODEFENCE: u32 = 1 << 11;
    pub const HTML: u32 = 1 << 12;
    pub const HIGHLIGHT: u32 = 1 << 13;
    pub const MATH: u32 = 1 << 14;
    pub const FOOTNOTE: u32 = 1 << 15;
    pub const TABLEHDR: u32 = 1 << 16;
    pub const FRONTMATTER: u32 = 1 << 17;
    /// Syntax characters: hidden in preview, dimmed in raw view.
    pub const MARKUP: u32 = 1 << 18;
    pub const HR: u32 = 1 << 19;
    pub const QUOTEBAR: u32 = 1 << 21;
    /// Obsidian `#tag`.
    pub const TAG: u32 = 1 << 20;
    pub const TABLEBORDER: u32 = 1 << 22;
    pub const HEADING_SHIFT: u32 = 24;
    pub const HEADING_MASK: u32 = 0b111 << HEADING_SHIFT;

    pub fn heading(level: u8) -> u32 {
        (level as u32) << HEADING_SHIFT
    }
    pub fn heading_level(f: u32) -> u8 {
        ((f & HEADING_MASK) >> HEADING_SHIFT) as u8
    }
}

use flags::*;

#[derive(Clone, Debug, PartialEq)]
pub enum LineKind {
    Normal,
    Heading(u8),
    FenceOpen(String),
    FenceClose,
    CodeBody,
    Hr,
    TableHeader(usize),
    TableDelim(usize),
    TableBody(usize),
    FrontMatter,
}

#[derive(Clone, Debug)]
pub struct Replacement {
    pub text: String,
    pub flags: u32,
    /// Bytes of source replaced (0 = pure insertion before the byte).
    pub len: usize,
}

#[derive(Clone, Debug, Default)]
pub struct TableInfo {
    pub aligns: Vec<Alignment>,
    pub widths: Vec<usize>,
    /// Cell byte ranges (absolute) per line.
    pub cells: HashMap<usize, Vec<Range<usize>>>,
}

pub struct MdDoc {
    pub text: String,
    pub line_starts: Vec<usize>,
    pub flags: Vec<u32>,
    pub replace: HashMap<usize, Replacement>,
    pub kinds: Vec<LineKind>,
    /// Inclusive line range of the block (table / code block / front matter)
    /// each line belongs to, used to reveal whole blocks at once.
    pub groups: Vec<Option<(usize, usize)>>,
    pub tables: Vec<TableInfo>,
    /// Heading line → last line of its section (for folding).
    pub section_ends: HashMap<usize, usize>,
}

struct Frame {
    range: Range<usize>,
    children: Vec<Range<usize>>,
}

// Obsidian uses the same bullet at every depth.
const BULLETS: [&str; 1] = ["•"];

impl MdDoc {
    pub fn parse(text: &str) -> MdDoc {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        let nlines = line_starts.len();
        let mut doc = MdDoc {
            text: text.to_string(),
            line_starts,
            flags: vec![0; text.len() + 1],
            replace: HashMap::new(),
            kinds: vec![LineKind::Normal; nlines],
            groups: vec![None; nlines],
            tables: Vec::new(),
            section_ends: HashMap::new(),
        };
        doc.analyse();
        doc.compute_table_widths();
        doc.compute_sections();
        doc
    }

    pub fn line_of(&self, byte: usize) -> usize {
        match self.line_starts.binary_search(&byte) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    pub fn line_range(&self, l: usize) -> Range<usize> {
        let start = self.line_starts[l];
        let end = if l + 1 < self.line_starts.len() { self.line_starts[l + 1] - 1 } else { self.text.len() };
        start..end
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn heading_level(&self, l: usize) -> Option<u8> {
        match self.kinds.get(l) {
            Some(LineKind::Heading(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn group(&self, l: usize) -> Option<(usize, usize)> {
        self.groups.get(l).copied().flatten()
    }

    fn add(&mut self, r: Range<usize>, f: u32) {
        let end = r.end.min(self.flags.len());
        for b in &mut self.flags[r.start.min(end)..end] {
            *b |= f;
        }
    }

    fn set_group(&mut self, r: &Range<usize>) {
        let l1 = self.line_of(r.start);
        let l2 = self.line_of(r.end.saturating_sub(1).max(r.start));
        for l in l1..=l2 {
            self.groups[l] = Some((l1, l2));
        }
    }

    fn analyse(&mut self) {
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_FOOTNOTES);
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TASKLISTS);
        opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
        opts.insert(Options::ENABLE_MATH);
        let events: Vec<(Event<'static>, Range<usize>)> =
            Parser::new_ext(&self.text, opts).into_offset_iter().map(|(e, r)| (e.into_static(), r)).collect();

        let mut frames: Vec<Frame> = Vec::new();
        let mut list_depth = 0usize;
        let mut quote_depth = 0usize;
        let mut in_code = false;
        let mut cur_table: Option<usize> = None;
        let mut in_head = false;
        // Byte range of the current list item's bullet (marker + one space).
        let mut item_markers: Vec<Option<Range<usize>>> = Vec::new();

        for (ev, r) in events {
            match ev {
                Event::Start(tag) => match tag {
                    Tag::Heading { level, .. } => self.heading(r, level),
                    Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. } => {
                        let f = match tag {
                            Tag::Emphasis => ITALIC,
                            Tag::Strong => BOLD,
                            Tag::Strikethrough => STRIKE,
                            Tag::Link { .. } => LINK,
                            _ => IMAGE,
                        };
                        self.add(r.clone(), f);
                        if matches!(tag, Tag::Image { .. }) {
                            self.replace.insert(r.start, Replacement { text: "🖼 ".into(), flags: IMAGE, len: 0 });
                        }
                        frames.push(Frame { range: r, children: Vec::new() });
                    }
                    Tag::BlockQuote(_) => {
                        quote_depth += 1;
                        self.add(r.clone(), QUOTE);
                        if quote_depth == 1 {
                            self.quote_markers(&r);
                        }
                    }
                    Tag::CodeBlock(kind) => {
                        in_code = true;
                        self.code_block(&r, &kind);
                    }
                    Tag::List(_) => list_depth += 1,
                    Tag::Item => {
                        let m = self.list_marker(r.start, list_depth);
                        item_markers.push(m);
                    }
                    Tag::Table(aligns) => {
                        let idx = self.tables.len();
                        self.tables.push(TableInfo { aligns, ..Default::default() });
                        cur_table = Some(idx);
                        self.set_group(&r);
                        let l1 = self.line_of(r.start);
                        let l2 = self.line_of(r.end.saturating_sub(1));
                        for l in l1..=l2 {
                            self.kinds[l] = LineKind::TableBody(idx);
                        }
                        self.kinds[l1] = LineKind::TableHeader(idx);
                        if l1 < l2 {
                            self.kinds[l1 + 1] = LineKind::TableDelim(idx);
                        }
                    }
                    Tag::TableHead => in_head = true,
                    Tag::TableCell => {
                        if let Some(t) = cur_table {
                            let line = self.line_of(r.start);
                            self.tables[t].cells.entry(line).or_default().push(r.clone());
                            if in_head {
                                self.add(r.clone(), TABLEHDR);
                            }
                        }
                    }
                    Tag::MetadataBlock(_) => {
                        self.add(r.clone(), FRONTMATTER);
                        self.set_group(&r);
                        let l1 = self.line_of(r.start);
                        let l2 = self.line_of(r.end.saturating_sub(1));
                        for l in l1..=l2 {
                            self.kinds[l] = LineKind::FrontMatter;
                        }
                    }
                    Tag::HtmlBlock => self.add(r, HTML),
                    Tag::FootnoteDefinition(_) => {
                        let line = self.line_range(self.line_of(r.start));
                        if let Some(p) = self.text[r.start..line.end].find("]:") {
                            self.add(r.start..r.start + p + 2, FOOTNOTE);
                        }
                    }
                    _ => {}
                },
                Event::End(end) => match end {
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link | TagEnd::Image => {
                        if let Some(f) = frames.pop() {
                            self.mark_gaps(&f);
                            if let Some(parent) = frames.last_mut() {
                                parent.children.push(f.range);
                            }
                        }
                    }
                    TagEnd::BlockQuote(_) => quote_depth = quote_depth.saturating_sub(1),
                    TagEnd::CodeBlock => in_code = false,
                    TagEnd::List(_) => list_depth = list_depth.saturating_sub(1),
                    TagEnd::Item => {
                        item_markers.pop();
                    }
                    TagEnd::Table => cur_table = None,
                    TagEnd::TableHead => in_head = false,
                    _ => {}
                },
                Event::Text(_) => {
                    if !in_code {
                        self.highlights(&r);
                        self.escapes(&r);
                        self.tags(&r);
                    }
                    if let Some(f) = frames.last_mut() {
                        f.children.push(r);
                    }
                }
                Event::Code(_) => {
                    self.inline_code(&r);
                    if let Some(f) = frames.last_mut() {
                        f.children.push(r);
                    }
                }
                Event::InlineMath(_) | Event::DisplayMath(_) => {
                    let s = &self.text[r.clone()];
                    let n = s.bytes().take_while(|&b| b == b'$').count();
                    let len = s.len();
                    self.add(r.clone(), MATH);
                    if len >= 2 * n {
                        self.add(r.start..r.start + n, MARKUP);
                        self.add(r.end - n..r.end, MARKUP);
                    }
                    if let Some(f) = frames.last_mut() {
                        f.children.push(r);
                    }
                }
                Event::Html(_) | Event::InlineHtml(_) => {
                    self.add(r.clone(), HTML);
                    if let Some(f) = frames.last_mut() {
                        f.children.push(r);
                    }
                }
                Event::FootnoteReference(label) => {
                    let sup: Option<String> = label.chars().map(superscript).collect();
                    let text = sup.unwrap_or_else(|| format!("[{label}]"));
                    self.replace.insert(r.start, Replacement { text, flags: FOOTNOTE, len: r.len() });
                    if let Some(f) = frames.last_mut() {
                        f.children.push(r);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some(f) = frames.last_mut() {
                        f.children.push(r);
                    }
                }
                Event::Rule => {
                    let l = self.line_of(r.start);
                    self.kinds[l] = LineKind::Hr;
                    self.add(r, HR);
                }
                Event::TaskListMarker(done) => {
                    let text = if done { "☑" } else { "☐" };
                    self.replace.insert(r.start, Replacement { text: text.into(), flags: TASKBOX, len: r.len() });
                    // Task items show only the checkbox: hide the bullet.
                    if let Some(Some(m)) = item_markers.last() {
                        self.replace.remove(&m.start);
                        self.add(m.clone(), MARKUP);
                    }
                    if done {
                        let line = self.line_range(self.line_of(r.start));
                        self.add(r.end..line.end, TASKDONE);
                    }
                }
            }
        }
    }

    fn heading(&mut self, r: Range<usize>, level: HeadingLevel) {
        let lvl = level as u8;
        self.add(r.clone(), heading(lvl));
        let l1 = self.line_of(r.start);
        let l2 = self.line_of(r.end.saturating_sub(1).max(r.start));
        self.kinds[l1] = LineKind::Heading(lvl);
        let lr = self.line_range(l1);
        let line = self.text[lr.clone()].to_string();
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            // ATX: hide "## " and an optional closing "###".
            let lead = line.len() - trimmed.len();
            let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
            let spaces = trimmed[hashes..].bytes().take_while(|&b| b == b' ' || b == b'\t').count();
            self.add(lr.start..lr.start + lead + hashes + spaces, MARKUP);
            let body = line.trim_end();
            let closing = body.bytes().rev().take_while(|&b| b == b'#').count();
            if closing > 0 && closing < body.len() - lead - hashes {
                let before = &body[..body.len() - closing];
                if before.ends_with(' ') || before.ends_with('\t') {
                    let ws = before.len() - before.trim_end().len();
                    self.add(lr.start + body.len() - closing - ws..lr.start + body.len(), MARKUP);
                }
            }
        } else if l2 > l1 {
            // Setext: the underline line is markup; the heading spans the text lines.
            for l in l1..l2 {
                self.kinds[l] = LineKind::Heading(lvl);
            }
            let ur = self.line_range(l2);
            self.add(ur, MARKUP);
        }
    }

    fn quote_markers(&mut self, r: &Range<usize>) {
        let l1 = self.line_of(r.start);
        let l2 = self.line_of(r.end.saturating_sub(1).max(r.start));
        for l in l1..=l2 {
            let lr = self.line_range(l);
            let bytes = self.text.as_bytes()[..lr.end].to_vec();
            let mut i = lr.start;
            while i < lr.end && bytes[i] == b' ' {
                i += 1;
            }
            while i < lr.end && bytes[i] == b'>' {
                let len = if i + 1 < lr.end && bytes[i + 1] == b' ' { 2 } else { 1 };
                self.replace.insert(i, Replacement { text: "▎ ".into(), flags: QUOTEBAR, len });
                self.add(i..i + len, QUOTEBAR);
                i += len;
                while i < lr.end && bytes[i] == b' ' {
                    i += 1;
                }
            }
        }
    }

    fn code_block(&mut self, r: &Range<usize>, kind: &CodeBlockKind) {
        self.add(r.clone(), CODEBLOCK);
        self.set_group(r);
        let l1 = self.line_of(r.start);
        let mut l2 = self.line_of(r.end.saturating_sub(1).max(r.start));
        // pulldown includes the trailing newline of the closing fence.
        if l2 > l1 && self.line_starts[l2] == r.end {
            l2 -= 1;
        }
        match kind {
            CodeBlockKind::Fenced(lang) => {
                self.kinds[l1] = LineKind::FenceOpen(lang.to_string());
                self.add(self.line_range(l1), CODEFENCE);
                for l in l1 + 1..=l2 {
                    self.kinds[l] = LineKind::CodeBody;
                }
                if l2 > l1 {
                    let last = self.text[self.line_range(l2)].trim_start();
                    if last.starts_with("```") || last.starts_with("~~~") {
                        self.kinds[l2] = LineKind::FenceClose;
                        self.add(self.line_range(l2), CODEFENCE);
                    }
                }
                for l in l1..=l2 {
                    self.groups[l] = Some((l1, l2));
                }
            }
            CodeBlockKind::Indented => {
                for l in l1..=l2 {
                    self.kinds[l] = LineKind::CodeBody;
                    self.groups[l] = Some((l1, l2));
                }
            }
        }
    }

    fn list_marker(&mut self, start: usize, depth: usize) -> Option<Range<usize>> {
        let bytes = self.text.as_bytes();
        let b = *bytes.get(start)?;
        let next = bytes.get(start + 1).copied();
        let digits = bytes[start..].iter().take_while(|b| b.is_ascii_digit()).count();
        if b == b'-' || b == b'*' || b == b'+' {
            let len = if next == Some(b' ') { 2 } else { 1 };
            let bullet = BULLETS[(depth.saturating_sub(1)) % BULLETS.len()];
            self.replace.insert(start, Replacement { text: bullet.into(), flags: LISTMARK, len: 1 });
            self.add(start..start + 1, LISTMARK);
            Some(start..start + len)
        } else if digits > 0 {
            self.add(start..start + digits + 1, LISTMARK);
            None
        } else {
            None
        }
    }

    fn mark_gaps(&mut self, f: &Frame) {
        let mut children = f.children.clone();
        children.sort_by_key(|c| c.start);
        let mut pos = f.range.start;
        let mut gaps = Vec::new();
        for c in &children {
            if c.start > pos {
                gaps.push(pos..c.start);
            }
            pos = pos.max(c.end);
        }
        if pos < f.range.end {
            gaps.push(pos..f.range.end);
        }
        for g in gaps {
            // Never hide newlines: they delimit lines.
            let s = self.text[g.clone()].to_string();
            for (i, ch) in s.char_indices() {
                if ch != '\n' {
                    self.add(g.start + i..g.start + i + ch.len_utf8(), MARKUP);
                }
            }
        }
    }

    fn inline_code(&mut self, r: &Range<usize>) {
        let s = &self.text[r.clone()];
        let n = s.bytes().take_while(|&b| b == b'`').count();
        let m = s.bytes().rev().take_while(|&b| b == b'`').count();
        let len = s.len();
        self.add(r.clone(), CODE);
        if n + m <= len {
            self.add(r.start..r.start + n, MARKUP);
            self.add(r.end - m..r.end, MARKUP);
        }
    }

    /// `==highlight==` (Obsidian extension) inside a text run.
    fn highlights(&mut self, r: &Range<usize>) {
        let s = self.text[r.clone()].to_string();
        let mut search = 0;
        while let Some(a) = s[search..].find("==") {
            let a = search + a;
            let Some(b) = s[a + 2..].find("==") else { break };
            let b = a + 2 + b;
            if b > a + 2 {
                self.add(r.start + a..r.start + a + 2, MARKUP);
                self.add(r.start + a + 2..r.start + b, HIGHLIGHT);
                self.add(r.start + b..r.start + b + 2, MARKUP);
            }
            search = b + 2;
        }
    }

    /// Obsidian tags: `#word` preceded by whitespace (or a text-run start),
    /// containing at least one non-digit.
    fn tags(&mut self, r: &Range<usize>) {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| regex::Regex::new(r"(?:^|[\s(])(#[\p{L}\p{N}_/-]*[\p{L}_/-][\p{L}\p{N}_/-]*)").unwrap());
        let s = self.text[r.clone()].to_string();
        let at_line_start = r.start == 0 || matches!(self.text.as_bytes()[r.start - 1], b'\n' | b' ' | b'\t' | b'(');
        for cap in re.captures_iter(&s) {
            let m = cap.get(1).unwrap();
            if m.start() == 0 && !at_line_start {
                continue;
            }
            self.add(r.start + m.start()..r.start + m.end(), TAG);
        }
    }

    /// Backslash escapes: pulldown reports the escaped char's text range
    /// starting after the backslash.
    fn escapes(&mut self, r: &Range<usize>) {
        if r.start > 0 && self.text.as_bytes()[r.start - 1] == b'\\' {
            let c = self.text[r.start..].chars().next();
            if c.is_some_and(|c| c.is_ascii_punctuation()) {
                self.add(r.start - 1..r.start, MARKUP);
            }
        }
    }

    /// One pass with a stack of open headings: a heading's section ends
    /// before the next heading of the same or higher level. Trailing blank
    /// lines stay outside the fold so sections remain visually separated.
    fn compute_sections(&mut self) {
        let n = self.line_count();
        let mut stack: Vec<(usize, u8)> = Vec::new();
        let close = |start: usize, next: usize, doc: &MdDoc, out: &mut HashMap<usize, usize>| {
            let mut end = next - 1;
            if next < n {
                while end > start && doc.line_range(end).is_empty() {
                    end -= 1;
                }
            }
            out.insert(start, end);
        };
        let mut out = HashMap::new();
        for l in 0..n {
            if let Some(lvl) = self.heading_level(l) {
                while let Some(&(s, sl)) = stack.last() {
                    if sl >= lvl {
                        close(s, l, self, &mut out);
                        stack.pop();
                    } else {
                        break;
                    }
                }
                stack.push((l, lvl));
            }
        }
        for (s, _) in stack {
            close(s, n, self, &mut out);
        }
        self.section_ends = out;
    }

    fn compute_table_widths(&mut self) {
        for t in 0..self.tables.len() {
            let mut widths: Vec<usize> = vec![0; self.tables[t].aligns.len()];
            let rows: Vec<Vec<Range<usize>>> = self.tables[t].cells.values().cloned().collect();
            for row in rows {
                for (i, cell) in row.iter().enumerate() {
                    let w = render::cell_cells(self, cell.clone()).iter().map(|c| c.width as usize).sum();
                    if i >= widths.len() {
                        widths.push(0);
                    }
                    widths[i] = widths[i].max(w);
                }
            }
            for w in &mut widths {
                *w = (*w).max(3);
            }
            self.tables[t].widths = widths;
        }
    }
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        _ => return None,
    })
}
