//! The headless editor core: a state machine driven by `handle_key`.

pub mod cmdline;
pub mod ex;
pub mod insert;
pub mod keymap;
pub mod layout;
pub mod motion;
pub mod mouse;
pub mod normal;
pub mod operator;
pub mod register;
pub mod search;
pub mod textobj;
pub mod view;
pub mod visual;
pub mod window;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};

use crate::buffer::Buffer;
use crate::key::{Key, KeyCode};
use crate::options::Options;
use crate::theme::Theme;
use crate::types::{BufId, Pos, Rect, WinId};

use cmdline::CmdLine;
use keymap::{MapMatch, MapMode, Keymaps};
use layout::Node;
use normal::Cmd;
use register::Registers;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VKind {
    Char,
    Line,
    Block,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Replace,
    Visual(VKind),
    Cmdline,
}

impl Mode {
    pub fn is_visual(&self) -> bool {
        matches!(self, Mode::Visual(_))
    }
    pub fn is_insert(&self) -> bool {
        matches!(self, Mode::Insert | Mode::Replace)
    }
}

pub struct Window {
    pub id: WinId,
    pub buf: BufId,
    pub cursor: Pos,
    /// Desired column for vertical motions; `usize::MAX` means end of line.
    pub want_col: usize,
    pub top: usize,
    pub leftcol: usize,
    pub vstart: Pos,
    pub last_vkind: VKind,
    pub jumps: Vec<(BufId, Pos)>,
    pub jump_idx: usize,
    pub alt_buf: Option<BufId>,
    /// Window-local live-preview override (`:preview`).
    pub preview: Option<bool>,
}

impl Window {
    fn new(id: WinId, buf: BufId) -> Self {
        Window {
            id,
            buf,
            cursor: Pos::default(),
            want_col: 0,
            top: 0,
            leftcol: 0,
            vstart: Pos::default(),
            last_vkind: VKind::Char,
            jumps: Vec::new(),
            jump_idx: 0,
            alt_buf: None,
            preview: None,
        }
    }
}

pub struct Tab {
    pub root: Node,
    pub active: WinId,
    pub prev: Option<WinId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgKind {
    Info,
    Error,
}

#[derive(Default)]
pub struct InsertState {
    pub start: Pos,
    pub keys: Vec<Key>,
    pub count: usize,
    /// 'o'/'O' when the insert opened a line (count repeats reopen lines).
    pub open_kind: Option<char>,
    pub block: Option<BlockInsert>,
    pub replaced: Vec<Option<char>>,
    pub completion: Option<Completion>,
    pub ctrl_o: bool,
    pub awaiting: Option<char>,
    pub replaying: bool,
}

#[derive(Clone, Debug)]
pub struct BlockInsert {
    pub first: usize,
    pub last: usize,
    pub col: usize,
    pub append_eol: bool,
    pub pad: bool,
}

#[derive(Clone, Debug)]
pub struct Completion {
    pub start_col: usize,
    pub prefix: String,
    pub items: Vec<String>,
    pub idx: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Dot {
    pub cmd: Cmd,
    pub insert_keys: Option<Vec<Key>>,
}

#[derive(Default)]
pub struct SearchState {
    pub pattern: Option<String>,
    pub forward: bool,
    pub hl: bool,
}

pub struct Editor {
    pub buffers: BTreeMap<BufId, Buffer>,
    pub windows: HashMap<WinId, Window>,
    pub tabs: Vec<Tab>,
    pub tab: usize,
    next_buf: BufId,
    next_win: WinId,
    pub mode: Mode,
    pub options: Options,
    pub theme: Theme,
    /// Group overrides from config `[theme]`, re-applied on `:colorscheme`.
    pub theme_overrides: Vec<(String, crate::theme::StyleSpec)>,
    pub leader: String,
    pub keymaps: Keymaps,
    pub registers: Registers,
    pub global_marks: HashMap<char, (BufId, Pos)>,
    pub search: SearchState,
    pub last_find: Option<(char, bool, bool)>,
    pub last_sub: Option<(String, String, String)>,
    pub cmdline: CmdLine,
    pub messages: Vec<(String, MsgKind)>,
    pub hit_enter: bool,
    pub pending: Vec<Key>,
    map_pending: Vec<Key>,
    typeahead: VecDeque<(Key, bool)>,
    expansions: usize,
    pub recording: Option<(char, Vec<Key>)>,
    pub last_macro: Option<char>,
    pub dot: Option<Dot>,
    pub dot_pending: Option<Cmd>,
    pub replaying_dot: bool,
    pub insert: InsertState,
    pub size: (u16, u16),
    pub quit: bool,
    pub suspend: bool,
    pub last_ex: Option<String>,
    pub cmd_history: Vec<String>,
    pub search_history: Vec<String>,
    /// Pending visual-mode dot info set by visual operators.
    pub last_visual_dot: Option<normal::VisualDot>,
    /// Drag-select origin for the mouse.
    pub mouse_drag: Option<(WinId, Pos)>,
    /// Nesting depth of ex commands; undo commits are deferred while > 0.
    pub batch: usize,
    /// The terminal supports the kitty text-sizing protocol (OSC 66).
    pub text_sizing: bool,
}

impl Editor {
    pub fn new(width: u16, height: u16) -> Self {
        let mut ed = Editor {
            buffers: BTreeMap::new(),
            windows: HashMap::new(),
            tabs: Vec::new(),
            tab: 0,
            next_buf: 1,
            next_win: 1000,
            mode: Mode::Normal,
            options: Options::default(),
            theme: Theme::default(),
            theme_overrides: Vec::new(),
            leader: " ".into(),
            keymaps: Keymaps::default(),
            registers: Registers::default(),
            global_marks: HashMap::new(),
            search: SearchState { forward: true, ..Default::default() },
            last_find: None,
            last_sub: None,
            cmdline: CmdLine::default(),
            messages: Vec::new(),
            hit_enter: false,
            pending: Vec::new(),
            map_pending: Vec::new(),
            typeahead: VecDeque::new(),
            expansions: 0,
            recording: None,
            last_macro: None,
            dot: None,
            dot_pending: None,
            replaying_dot: false,
            insert: InsertState::default(),
            size: (width, height),
            quit: false,
            suspend: false,
            last_ex: None,
            cmd_history: Vec::new(),
            search_history: Vec::new(),
            last_visual_dot: None,
            mouse_drag: None,
            batch: 0,
            text_sizing: false,
        };
        let b = ed.new_buffer();
        let w = ed.new_window(b);
        ed.tabs.push(Tab { root: Node::Leaf(w), active: w, prev: None });
        ed.default_keymaps();
        ed
    }

    fn default_keymaps(&mut self) {
        let leader = self.leader.clone();
        self.keymaps.add(MapMode::Normal, "<leader>x", ":task<CR>", true, &leader);
        self.keymaps.add(MapMode::Visual, "<leader>x", ":task<CR>", true, &leader);
    }

    // --- Buffers & windows ---------------------------------------------------

    pub fn new_buffer(&mut self) -> BufId {
        let id = self.next_buf;
        self.next_buf += 1;
        self.buffers.insert(id, Buffer::empty(id));
        id
    }

    pub fn new_window(&mut self, buf: BufId) -> WinId {
        let id = self.next_win;
        self.next_win += 1;
        self.windows.insert(id, Window::new(id, buf));
        id
    }

    /// Find or load a buffer for `path`.
    pub fn buffer_for_path(&mut self, path: &Path) -> Result<BufId, String> {
        let canon = canonical(path);
        if let Some(b) = self.buffers.values().find(|b| b.path.as_ref().map(|p| canonical(p)) == Some(canon.clone())) {
            return Ok(b.id);
        }
        let id = self.next_buf;
        let (buf, exists) = Buffer::open(id, path).map_err(|e| format!("\"{}\": {e}", path.display()))?;
        self.next_buf += 1;
        // Reuse a pristine empty buffer (the startup [No Name]) if it is unshown.
        self.buffers.insert(id, buf);
        let lines = self.buffers[&id].line_count();
        if exists {
            let chars = self.buffers[&id].len_chars();
            self.info(format!("\"{}\" {}L, {}C", path.display(), lines, chars));
        } else {
            self.info(format!("\"{}\" [New]", path.display()));
        }
        Ok(id)
    }

    pub fn tab(&self) -> &Tab {
        &self.tabs[self.tab]
    }
    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.tab]
    }
    pub fn win_id(&self) -> WinId {
        self.tabs[self.tab].active
    }
    pub fn win(&self) -> &Window {
        &self.windows[&self.win_id()]
    }
    pub fn win_mut(&mut self) -> &mut Window {
        let id = self.win_id();
        self.windows.get_mut(&id).unwrap()
    }
    pub fn buf(&self) -> &Buffer {
        &self.buffers[&self.win().buf]
    }
    pub fn buf_mut(&mut self) -> &mut Buffer {
        let id = self.win().buf;
        self.buffers.get_mut(&id).unwrap()
    }
    pub fn cursor(&self) -> Pos {
        self.win().cursor
    }
    pub fn set_cursor(&mut self, p: Pos) {
        self.win_mut().cursor = p;
        self.clamp_cursor();
    }
    /// Set cursor and remember its column for vertical motions.
    pub fn set_cursor_col(&mut self, p: Pos) {
        self.set_cursor(p);
        let c = self.cursor().col;
        self.win_mut().want_col = c;
    }

    pub fn clamp_cursor(&mut self) {
        let mode = self.mode;
        let id = self.win_id();
        let w = self.windows.get_mut(&id).unwrap();
        let b = &self.buffers[&w.buf];
        w.cursor.line = w.cursor.line.min(b.line_count() - 1);
        let len = b.line_len(w.cursor.line);
        let max = match mode {
            Mode::Insert | Mode::Replace => len,
            Mode::Visual(_) if w.want_col == usize::MAX => len,
            _ if self.insert.ctrl_o => len,
            _ => len.saturating_sub(1),
        };
        w.cursor.col = w.cursor.col.min(max);
    }

    pub fn preview_on(&self, win: &Window) -> bool {
        self.buffers[&win.buf].is_markdown() && win.preview.unwrap_or(self.options.preview)
    }

    // --- Messages -----------------------------------------------------------

    /// Switch colour scheme, keeping the user's group overrides on top.
    pub fn set_colorscheme(&mut self, name: &str) -> Result<(), String> {
        self.theme = crate::theme::load(name, crate::theme::user_dir().as_deref())?;
        let errs = self.apply_theme_overrides();
        match errs.first() {
            Some(e) => Err(format!("theme.{e}")),
            None => Ok(()),
        }
    }

    /// Apply `theme_overrides` to the current scheme; returns errors.
    pub fn apply_theme_overrides(&mut self) -> Vec<String> {
        let overrides = std::mem::take(&mut self.theme_overrides);
        let errs = self.theme.apply_specs(overrides.iter().map(|(g, s)| (g, s)));
        self.theme_overrides = overrides;
        errs
    }

    pub fn info(&mut self, s: impl Into<String>) {
        self.messages = vec![(s.into(), MsgKind::Info)];
    }
    pub fn error(&mut self, s: impl Into<String>) {
        self.messages = vec![(s.into(), MsgKind::Error)];
    }
    /// Show multiple lines with a hit-enter prompt.
    pub fn show_lines(&mut self, lines: Vec<String>) {
        if lines.len() <= 1 {
            self.messages = lines.into_iter().map(|l| (l, MsgKind::Info)).collect();
            return;
        }
        self.messages = lines.into_iter().map(|l| (l, MsgKind::Info)).collect();
        self.hit_enter = true;
    }

    // --- Undo transactions --------------------------------------------------

    pub fn begin_change(&mut self) {
        let c = self.cursor();
        self.buf_mut().history.begin(c);
    }
    pub fn commit(&mut self) {
        if self.batch > 0 {
            return;
        }
        for b in self.buffers.values_mut() {
            b.history.commit();
        }
    }

    /// Insert text at char index in the current buffer.
    pub fn ins(&mut self, at: usize, s: &str) {
        let c = self.cursor();
        self.buf_mut().insert(at, s, c);
    }
    /// Remove [a, b) from the current buffer.
    pub fn del(&mut self, a: usize, b: usize) -> String {
        let c = self.cursor();
        self.buf_mut().remove(a, b, c)
    }
    pub fn set_line(&mut self, l: usize, s: &str) {
        let (a, b) = (self.buf().line_start(l), self.buf().line_end(l));
        if self.buf().slice(a, b) != s {
            self.del(a, b);
            self.ins(a, s);
        }
    }

    // --- Jumps & marks ------------------------------------------------------

    pub fn push_jump(&mut self) {
        let buf = self.win().buf;
        let pos = self.cursor();
        self.buf_mut().marks.insert('\'', pos);
        let w = self.win_mut();
        w.jumps.truncate(w.jump_idx);
        w.jumps.retain(|(b, p)| !(*b == buf && p.line == pos.line));
        w.jumps.push((buf, pos));
        if w.jumps.len() > 100 {
            w.jumps.remove(0);
        }
        w.jump_idx = w.jumps.len();
    }

    pub fn jump_older(&mut self, count: usize) {
        let (buf, pos) = (self.win().buf, self.cursor());
        {
            let w = self.win_mut();
            if w.jump_idx == w.jumps.len() {
                w.jumps.retain(|(b, p)| !(*b == buf && p.line == pos.line));
                w.jumps.push((buf, pos));
                w.jump_idx = w.jumps.len() - 1;
            }
        }
        let idx = self.win().jump_idx;
        if idx < count {
            return;
        }
        self.goto_jump(idx - count);
    }

    pub fn jump_newer(&mut self, count: usize) {
        let idx = self.win().jump_idx + count;
        if idx < self.win().jumps.len() {
            self.goto_jump(idx);
        }
    }

    fn goto_jump(&mut self, idx: usize) {
        let (b, p) = self.win().jumps[idx];
        self.win_mut().jump_idx = idx;
        if !self.buffers.contains_key(&b) {
            return;
        }
        if b != self.win().buf {
            self.switch_buffer(b);
        }
        self.set_cursor_col(p);
    }

    pub fn get_mark(&self, c: char) -> Option<(BufId, Pos)> {
        match c {
            'A'..='Z' | '0'..='9' => self.global_marks.get(&c).copied(),
            '`' => self.buf().marks.get(&'\'').map(|p| (self.win().buf, *p)),
            _ => self.buf().marks.get(&c).map(|p| (self.win().buf, *p)),
        }
    }

    pub fn set_mark(&mut self, c: char) -> bool {
        let pos = self.cursor();
        match c {
            'a'..='z' | '\'' | '`' | '[' | ']' | '<' | '>' => {
                let c = if c == '`' { '\'' } else { c };
                self.buf_mut().marks.insert(c, pos);
            }
            'A'..='Z' => {
                let b = self.win().buf;
                self.global_marks.insert(c, (b, pos));
            }
            _ => return false,
        }
        true
    }

    pub fn switch_buffer(&mut self, id: BufId) {
        let cur = self.cursor();
        let old = self.win().buf;
        if old == id {
            return;
        }
        self.commit();
        self.buf_mut().last_cursor = cur;
        let w = self.win_mut();
        w.alt_buf = Some(old);
        w.buf = id;
        w.top = 0;
        w.leftcol = 0;
        let p = self.buffers[&id].last_cursor;
        self.set_cursor_col(p);
        self.scroll_to_cursor_center_if_needed();
    }

    // --- Input pipeline -----------------------------------------------------

    /// Entry point for keys typed by the user.
    pub fn handle_key(&mut self, key: Key) {
        if let Some((_, rec)) = &mut self.recording {
            rec.push(key);
        }
        self.expansions = 0;
        self.typeahead.push_back((key, true));
        self.run_typeahead();
    }

    pub fn has_pending_map(&self) -> bool {
        !self.map_pending.is_empty()
    }

    pub fn pending_keys(&self) -> Vec<Key> {
        let mut v = self.pending.clone();
        v.extend(self.map_pending.iter().copied());
        v
    }

    /// Mapping timeout expired: resolve whatever is pending.
    pub fn on_timeout(&mut self) {
        if self.map_pending.is_empty() {
            return;
        }
        self.resolve_map_pending(true);
        self.run_typeahead();
    }

    fn map_mode(&self) -> Option<MapMode> {
        match self.mode {
            Mode::Insert | Mode::Replace => {
                if self.insert.awaiting.is_some() {
                    None
                } else {
                    Some(MapMode::Insert)
                }
            }
            Mode::Cmdline => None,
            Mode::Normal | Mode::Visual(_) => {
                let visual = self.mode.is_visual();
                match normal::pending_state(&self.pending) {
                    normal::PendState::Start => Some(if visual { MapMode::Visual } else { MapMode::Normal }),
                    normal::PendState::Operator if !visual => Some(MapMode::Operator),
                    _ => None,
                }
            }
        }
    }

    fn run_typeahead(&mut self) {
        while let Some((key, remap)) = self.typeahead.pop_front() {
            if self.quit {
                self.typeahead.clear();
                return;
            }
            let mode = if remap || !self.map_pending.is_empty() { self.map_mode() } else { None };
            match mode {
                Some(mm) if remap => {
                    self.map_pending.push(key);
                    match self.keymaps.lookup(mm, &self.map_pending) {
                        MapMatch::Prefix | MapMatch::Exact(_, true) => {
                            // Wait for more keys (or the timeout).
                            continue;
                        }
                        MapMatch::Exact(m, false) => {
                            self.map_pending.clear();
                            self.expand(m.rhs, !m.noremap);
                        }
                        MapMatch::None => self.resolve_map_pending(false),
                    }
                }
                _ => {
                    if !self.map_pending.is_empty() {
                        self.typeahead.push_front((key, remap));
                        self.resolve_map_pending(true);
                        continue;
                    }
                    self.process_key(key);
                }
            }
        }
    }

    fn expand(&mut self, rhs: Vec<Key>, remap: bool) {
        self.expansions += 1;
        if self.expansions > 1000 {
            self.typeahead.clear();
            self.error("E223: Recursive mapping");
            return;
        }
        for k in rhs.into_iter().rev() {
            self.typeahead.push_front((k, remap));
        }
    }

    /// The pending keys no longer extend to a mapping: expand the longest
    /// exact prefix if any, else pass the first key through unmapped.
    fn resolve_map_pending(&mut self, allow_full: bool) {
        let pend = std::mem::take(&mut self.map_pending);
        let Some(mm) = self.map_mode() else {
            for (i, k) in pend.into_iter().enumerate() {
                self.typeahead.insert(i, (k, false));
            }
            return;
        };
        let max = if allow_full { pend.len() } else { pend.len() - 1 };
        for n in (1..=max).rev() {
            if let MapMatch::Exact(m, _) = self.keymaps.lookup(mm, &pend[..n]) {
                for (i, k) in pend[n..].iter().enumerate() {
                    self.typeahead.insert(i, (*k, true));
                }
                self.expand(m.rhs, !m.noremap);
                return;
            }
        }
        for (i, k) in pend[1..].iter().enumerate() {
            self.typeahead.insert(i, (*k, true));
        }
        self.process_key(pend[0]);
    }

    /// Run keys synchronously to completion (`:normal`, dot repeat of inserts).
    pub fn exec_keys(&mut self, keys: &[Key], remap: bool) {
        let saved = std::mem::take(&mut self.typeahead);
        let saved_pending = std::mem::take(&mut self.map_pending);
        for k in keys {
            self.typeahead.push_back((*k, remap));
        }
        self.run_typeahead();
        self.typeahead = saved;
        self.map_pending = saved_pending;
    }

    pub fn process_key(&mut self, key: Key) {
        if self.hit_enter {
            self.hit_enter = false;
            self.messages.clear();
            if matches!(key.code, KeyCode::Enter | KeyCode::Esc) || key.char() == Some(' ') {
                return;
            }
        }
        match self.mode {
            Mode::Normal | Mode::Visual(_) => self.normal_key(key),
            Mode::Insert | Mode::Replace => self.insert_key(key),
            Mode::Cmdline => self.cmdline_key(key),
        }
        if self.windows.is_empty() || self.quit {
            return;
        }
        self.clamp_cursor();
        self.scroll_to_cursor();
    }

    /// Stop macro recording and store it in the register.
    pub fn stop_recording(&mut self) {
        if let Some((reg, mut keys)) = self.recording.take() {
            keys.pop(); // the 'q' that stopped recording
            let text = crate::key::keys_to_register(&keys);
            let val = register::RegVal::new(text, register::RegKind::Char);
            // Uppercase registers append (handled by `yank`).
            self.registers.yank(Some(reg), val);
        }
    }

    pub fn text_rect(&self, id: WinId) -> Rect {
        let r = self.window_rect(id);
        let g = self.gutter_width(self.windows[&id].buf) as u16;
        Rect::new(r.x + g.min(r.w), r.y, r.w.saturating_sub(g), r.h.saturating_sub(1))
    }

    pub fn gutter_width(&self, buf: BufId) -> usize {
        if !(self.options.number || self.options.relativenumber) {
            return 0;
        }
        let n = self.buffers[&buf].line_count();
        n.to_string().len().max(3) + 1
    }

    /// Area for windows (screen minus tabline and command line).
    pub fn editor_area(&self) -> Rect {
        let tabline = if self.tabs.len() > 1 { 1 } else { 0 };
        Rect::new(0, tabline, self.size.0, self.size.1.saturating_sub(1 + tabline))
    }

    pub fn window_rect(&self, id: WinId) -> Rect {
        layout::rects(&self.tab().root, self.editor_area())
            .into_iter()
            .find(|(w, _)| *w == id)
            .map(|(_, r)| r)
            .unwrap_or_default()
    }

    pub fn resize(&mut self, w: u16, h: u16) {
        self.size = (w, h);
        let ids: Vec<WinId> = self.windows.keys().copied().collect();
        let cur = self.win_id();
        for id in ids {
            if layout::contains(&self.tab().root, id) {
                self.tab_mut().active = id;
                self.scroll_to_cursor();
            }
        }
        self.tab_mut().active = cur;
    }

    pub fn file_info(&self) -> String {
        let b = self.buf();
        let lines = b.line_count();
        let cur = self.cursor().line + 1;
        format!(
            "\"{}\"{} {} line{} --{}%--",
            b.display_name(),
            if b.is_modified() { " [Modified]" } else { "" },
            lines,
            if lines == 1 { "" } else { "s" },
            cur * 100 / lines.max(1)
        )
    }

    pub fn any_modified(&self) -> Option<&Buffer> {
        self.buffers.values().find(|b| b.is_modified())
    }

    pub fn open_initial(&mut self, paths: &[PathBuf]) {
        let first = self.win().buf;
        for (i, p) in paths.iter().enumerate() {
            match self.buffer_for_path(p) {
                Ok(id) => {
                    if i == 0 {
                        self.win_mut().buf = id;
                        self.buffers.remove(&first);
                    }
                }
                Err(e) => self.error(e),
            }
        }
    }
}

pub fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.to_path_buf())
        }
    })
}

#[cfg(test)]
pub mod tests;
