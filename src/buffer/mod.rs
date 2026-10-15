//! Text buffer: a rope holding the file's lines *without* the final newline,
//! plus undo history, file metadata, marks and heading folds.

pub mod history;

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ropey::Rope;

use crate::markdown::MdDoc;
use crate::types::{BufId, Pos};
use history::{Edit, History};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileType {
    Markdown,
    Text,
}

impl FileType {
    pub fn detect(path: Option<&Path>) -> FileType {
        match path.and_then(|p| p.extension()).and_then(|e| e.to_str()) {
            Some(e) if matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown" | "mkd" | "mdx") => {
                FileType::Markdown
            }
            _ => FileType::Text,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            FileType::Markdown => "markdown",
            FileType::Text => "text",
        }
    }
}

pub struct Buffer {
    pub id: BufId,
    pub text: Rope,
    pub path: Option<PathBuf>,
    pub filetype: FileType,
    pub crlf: bool,
    pub history: History,
    /// Bumped on every text change; used to invalidate caches.
    pub version: u64,
    /// Local marks a-z plus special marks (`.`, `^`, `<`, `>`, `"`, `[`, `]`).
    pub marks: HashMap<char, Pos>,
    /// Closed heading folds, keyed by the heading's line.
    pub folds: BTreeSet<usize>,
    /// Cursor to restore when the buffer is re-entered.
    pub last_cursor: Pos,
    md_cache: RefCell<Option<(u64, Rc<MdDoc>)>>,
}

impl Buffer {
    pub fn empty(id: BufId) -> Self {
        Buffer {
            id,
            text: Rope::new(),
            path: None,
            // Unnamed buffers are markdown; opened files use their extension.
            filetype: FileType::Markdown,
            crlf: false,
            history: History::new(),
            version: 0,
            marks: HashMap::new(),
            folds: BTreeSet::new(),
            last_cursor: Pos::default(),
            md_cache: RefCell::new(None),
        }
    }

    #[cfg(test)]
    pub fn from_str(id: BufId, s: &str) -> Self {
        let mut b = Buffer::empty(id);
        b.set_contents(s);
        b
    }

    /// Open `path`; a missing file yields an empty buffer bound to that path.
    pub fn open(id: BufId, path: &Path) -> anyhow::Result<(Self, bool)> {
        let mut b = Buffer::empty(id);
        b.path = Some(path.to_path_buf());
        b.filetype = FileType::detect(Some(path));
        let exists = path.exists();
        if exists {
            b.reload()?;
        }
        Ok((b, exists))
    }

    pub fn reload(&mut self) -> anyhow::Result<()> {
        let path = self.path.clone().ok_or_else(|| anyhow::anyhow!("No file name"))?;
        let bytes = fs::read(&path)?;
        let s = String::from_utf8_lossy(&bytes).into_owned();
        self.set_contents(&s);
        self.history.clear();
        self.folds.clear();
        Ok(())
    }

    fn set_contents(&mut self, s: &str) {
        self.crlf = s.contains("\r\n");
        let mut s = if self.crlf { s.replace("\r\n", "\n") } else { s.to_string() };
        if s.ends_with('\n') {
            s.pop();
        }
        self.text = Rope::from_str(&s);
        self.version += 1;
    }

    pub fn save(&mut self, path: Option<&Path>) -> anyhow::Result<(usize, usize)> {
        let target = match path {
            Some(p) => p.to_path_buf(),
            None => self.path.clone().ok_or_else(|| anyhow::anyhow!("E32: No file name"))?,
        };
        let mut out = self.text.to_string();
        if !(out.is_empty() && self.text.len_lines() == 1) {
            out.push('\n');
        }
        if self.crlf {
            out = out.replace('\n', "\r\n");
        }
        write_atomic(&target, out.as_bytes())?;
        if self.path.is_none() || path.is_none() || self.path.as_deref() == Some(&target) {
            if self.path.is_none() {
                self.filetype = FileType::detect(Some(&target));
            }
            self.path = Some(target);
            self.history.mark_saved();
        }
        Ok((self.line_count(), out.len()))
    }

    pub fn display_name(&self) -> String {
        match &self.path {
            Some(p) => p.display().to_string(),
            None => "[No Name]".into(),
        }
    }

    pub fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    pub fn is_markdown(&self) -> bool {
        self.filetype == FileType::Markdown
    }

    // --- Line/position helpers ------------------------------------------------

    pub fn line_count(&self) -> usize {
        self.text.len_lines()
    }

    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }

    /// Line content without the trailing newline.
    pub fn line(&self, l: usize) -> String {
        if l >= self.line_count() {
            return String::new();
        }
        let mut s = self.text.line(l).to_string();
        if s.ends_with('\n') {
            s.pop();
        }
        s
    }

    pub fn line_len(&self, l: usize) -> usize {
        if l >= self.line_count() {
            return 0;
        }
        let sl = self.text.line(l);
        let n = sl.len_chars();
        if n > 0 && sl.char(n - 1) == '\n' { n - 1 } else { n }
    }

    pub fn line_start(&self, l: usize) -> usize {
        self.text.line_to_char(l.min(self.line_count()))
    }

    /// Char index of the end of line `l` (position of its newline).
    pub fn line_end(&self, l: usize) -> usize {
        self.line_start(l) + self.line_len(l)
    }

    pub fn pos_to_char(&self, p: Pos) -> usize {
        let l = p.line.min(self.line_count() - 1);
        self.line_start(l) + p.col.min(self.line_len(l))
    }

    pub fn char_to_pos(&self, idx: usize) -> Pos {
        let idx = idx.min(self.len_chars());
        let line = self.text.char_to_line(idx);
        Pos::new(line, idx - self.text.line_to_char(line))
    }

    pub fn char_at(&self, idx: usize) -> Option<char> {
        if idx < self.len_chars() { Some(self.text.char(idx)) } else { None }
    }

    pub fn slice(&self, start: usize, end: usize) -> String {
        let end = end.min(self.len_chars());
        if start >= end {
            return String::new();
        }
        self.text.slice(start..end).to_string()
    }

    pub fn first_non_blank(&self, l: usize) -> usize {
        self.line(l).chars().take_while(|c| *c == ' ' || *c == '\t').count()
    }

    pub fn indent_of(&self, l: usize) -> String {
        self.line(l).chars().take_while(|c| *c == ' ' || *c == '\t').collect()
    }

    // --- Editing ------------------------------------------------------------

    pub fn insert(&mut self, at: usize, s: &str, cursor: Pos) {
        if s.is_empty() {
            return;
        }
        let at = at.min(self.len_chars());
        self.raw_insert(at, s);
        self.history.record(Edit { at, removed: String::new(), inserted: s.to_string() }, cursor);
    }

    pub fn remove(&mut self, start: usize, end: usize, cursor: Pos) -> String {
        let end = end.min(self.len_chars());
        if start >= end {
            return String::new();
        }
        let removed = self.slice(start, end);
        self.raw_remove(start, end);
        self.history.record(Edit { at: start, removed: removed.clone(), inserted: String::new() }, cursor);
        removed
    }

    fn raw_insert(&mut self, at: usize, s: &str) {
        let line = self.text.char_to_line(at);
        let added = s.matches('\n').count();
        self.text.insert(at, s);
        self.version += 1;
        if added > 0 {
            // Content after `at` on `line` moves down; marks on later lines shift.
            let col = at - self.text.line_to_char(line);
            self.shift_lines(line, col, added as isize);
        }
        self.marks.insert('.', self.char_to_pos(at));
    }

    fn raw_remove(&mut self, start: usize, end: usize) {
        let l1 = self.text.char_to_line(start);
        let l2 = self.text.char_to_line(end);
        self.text.remove(start..end);
        self.version += 1;
        if l2 > l1 {
            let removed = l2 - l1;
            let mut new_marks = HashMap::new();
            for (k, p) in self.marks.drain() {
                if p.line > l2 {
                    new_marks.insert(k, Pos::new(p.line - removed, p.col));
                } else if p.line > l1 {
                    new_marks.insert(k, Pos::new(l1, 0));
                } else {
                    new_marks.insert(k, p);
                }
            }
            self.marks = new_marks;
            self.folds = self
                .folds
                .iter()
                .filter(|&&f| f <= l1 || f > l2)
                .map(|&f| if f > l2 { f - removed } else { f })
                .collect();
        }
        self.marks.insert('.', self.char_to_pos(start));
    }

    fn shift_lines(&mut self, line: usize, col: usize, delta: isize) {
        for p in self.marks.values_mut() {
            if p.line > line || (p.line == line && p.col >= col && col > 0) {
                p.line = (p.line as isize + delta) as usize;
            }
        }
        self.folds = self
            .folds
            .iter()
            .map(|&f| if f > line || (f == line && col == 0) { (f as isize + delta) as usize } else { f })
            .collect();
    }

    pub fn undo(&mut self) -> Option<Pos> {
        let t = self.history.pop_undo()?;
        for e in t.edits.iter().rev() {
            if !e.inserted.is_empty() {
                self.raw_remove(e.at, e.at + e.inserted.chars().count());
            }
            if !e.removed.is_empty() {
                self.raw_insert(e.at, &e.removed);
            }
        }
        let pos = t.cursor_before;
        self.history.push_redo(t);
        Some(pos)
    }

    pub fn redo(&mut self) -> Option<Pos> {
        let t = self.history.pop_redo()?;
        for e in t.edits.iter() {
            if !e.removed.is_empty() {
                self.raw_remove(e.at, e.at + e.removed.chars().count());
            }
            if !e.inserted.is_empty() {
                self.raw_insert(e.at, &e.inserted);
            }
        }
        let pos = t.edits.first().map(|e| self.char_to_pos(e.at)).unwrap_or(t.cursor_before);
        self.history.push_undo_keep_redo(t);
        Some(pos)
    }

    pub fn contents(&self) -> String {
        self.text.to_string()
    }

    /// Parsed markdown for the current text (cached per version).
    pub fn markdown(&self) -> Rc<MdDoc> {
        let mut cache = self.md_cache.borrow_mut();
        if let Some((v, doc)) = cache.as_ref()
            && *v == self.version
        {
            return doc.clone();
        }
        let doc = Rc::new(MdDoc::parse(&self.text.to_string()));
        *cache = Some((self.version, doc.clone()));
        doc
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    let is_symlink = fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false);
    if is_symlink {
        fs::write(path, data)?;
        return Ok(());
    }
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.mdvim-tmp-{}", std::process::id()));
    let res = (|| -> anyhow::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
        // Fall back to a direct write (e.g. directory not writable but file is).
        fs::write(path, data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_positions() {
        let b = Buffer::from_str(1, "ab\ncde\n");
        assert_eq!(b.line_count(), 2);
        assert_eq!(b.line(1), "cde");
        assert_eq!(b.line_len(0), 2);
        assert_eq!(b.pos_to_char(Pos::new(1, 1)), 4);
        assert_eq!(b.char_to_pos(4), Pos::new(1, 1));
    }

    #[test]
    fn undo_redo() {
        let mut b = Buffer::from_str(1, "hello");
        b.history.begin(Pos::new(0, 0));
        b.insert(5, " world", Pos::new(0, 0));
        b.remove(0, 1, Pos::new(0, 0));
        b.history.commit();
        assert_eq!(b.contents(), "ello world");
        assert!(b.is_modified());
        b.undo();
        assert_eq!(b.contents(), "hello");
        assert!(!b.is_modified());
        b.redo();
        assert_eq!(b.contents(), "ello world");
    }
}
