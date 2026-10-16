//! Command-line mode (`:`, `/`, `?`): editing, history, completion, incsearch.

use super::normal::{Act, Cmd, Motion, Target};
use super::{Editor, Mode};
use crate::key::{Key, KeyCode};
use crate::options::OPTION_NAMES;
use crate::types::Pos;

#[derive(Default)]
pub struct CmdLine {
    pub kind: char,
    pub text: String,
    /// Cursor position in chars.
    pub cursor: usize,
    hist_idx: Option<usize>,
    hist_prefix: String,
    pub origin: Pos,
    origin_top: usize,
    /// Operator waiting for a search pattern (`d/foo<CR>`).
    pub pending_cmd: Option<Cmd>,
    completion: Option<(Vec<String>, usize, usize)>,
    awaiting_reg: bool,
    /// Current incsearch match (start, end).
    pub inc_match: Option<(Pos, Pos)>,
}

pub const EX_COMMANDS: &[&str] = &[
    "write", "quit", "wq", "xit", "exit", "qall", "wall", "wqall", "xall", "update", "edit", "enew", "read", "saveas",
    "file", "substitute", "global", "vglobal", "delete", "yank", "put", "move", "copy", "join", "normal", "sort", "undo",
    "redo", "set", "setlocal", "map", "noremap", "nmap", "nnoremap", "vmap", "vnoremap", "xmap", "xnoremap", "imap",
    "inoremap", "omap", "onoremap", "unmap", "nunmap", "vunmap", "iunmap", "ounmap", "split", "vsplit", "new", "vnew",
    "close", "only", "tabnew", "tabedit", "tabnext", "tabprevious", "tabclose", "tabonly", "tabfirst", "tablast",
    "buffers", "ls", "files", "buffer", "bnext", "bprevious", "bdelete", "bwipeout", "bfirst", "blast", "registers",
    "display", "marks", "jumps", "nohlsearch", "preview", "task", "pwd", "cd", "help", "mark", "resize", "vertical",
    "retab", "messages", "colorscheme",
];

impl Editor {
    pub fn start_cmdline(&mut self, kind: char, initial: &str) {
        if self.mode.is_visual() {
            self.exit_visual();
        }
        self.mode = Mode::Cmdline;
        self.messages.clear();
        let origin = self.cursor();
        let top = self.win().top;
        let pending = self.cmdline.pending_cmd.take();
        self.cmdline = CmdLine {
            kind,
            text: initial.to_string(),
            cursor: initial.chars().count(),
            origin,
            origin_top: top,
            pending_cmd: pending,
            ..Default::default()
        };
    }

    fn cmdline_cancel(&mut self) {
        let c = &self.cmdline;
        if c.kind != ':' {
            let (o, t) = (c.origin, c.origin_top);
            self.win_mut().cursor = o;
            self.win_mut().top = t;
        }
        self.cmdline.pending_cmd = None;
        self.cmdline.inc_match = None;
        self.mode = Mode::Normal;
    }

    pub fn cmdline_key(&mut self, key: Key) {
        if self.cmdline.awaiting_reg {
            self.cmdline.awaiting_reg = false;
            let text = if key.is_ctrl('w') {
                self.word_under_cursor().map(|(w, _)| w)
            } else {
                key.char().and_then(|c| self.registers.get(c)).map(|v| v.text.replace('\n', "\r"))
            };
            if let Some(t) = text {
                self.cmdline_insert(&t);
            }
            return;
        }
        let is_tab = matches!(key.code, KeyCode::Tab | KeyCode::BackTab);
        if !is_tab {
            self.cmdline.completion = None;
        }
        let c = &mut self.cmdline;
        let len = c.text.chars().count();
        match key.code {
            KeyCode::Esc => return self.cmdline_cancel(),
            KeyCode::Char('c') if key.ctrl => return self.cmdline_cancel(),
            KeyCode::Enter => return self.cmdline_enter(),
            KeyCode::Char('m') | KeyCode::Char('j') if key.ctrl => return self.cmdline_enter(),
            KeyCode::Backspace | KeyCode::Char('h') if key.code == KeyCode::Backspace || key.ctrl => {
                if c.text.is_empty() {
                    return self.cmdline_cancel();
                }
                if c.cursor > 0 {
                    let b = byte_idx(&c.text, c.cursor - 1);
                    c.text.remove(b);
                    c.cursor -= 1;
                }
            }
            KeyCode::Delete => {
                if c.cursor < len {
                    let b = byte_idx(&c.text, c.cursor);
                    c.text.remove(b);
                }
            }
            KeyCode::Left => c.cursor = c.cursor.saturating_sub(1),
            KeyCode::Right => c.cursor = (c.cursor + 1).min(len),
            KeyCode::Home => c.cursor = 0,
            KeyCode::End => c.cursor = len,
            KeyCode::Char('b') if key.ctrl => c.cursor = 0,
            KeyCode::Char('e') if key.ctrl => c.cursor = len,
            KeyCode::Char('u') if key.ctrl => {
                let b = byte_idx(&c.text, c.cursor);
                c.text.replace_range(..b, "");
                c.cursor = 0;
            }
            KeyCode::Char('w') if key.ctrl => {
                let chars: Vec<char> = c.text.chars().collect();
                let mut i = c.cursor;
                while i > 0 && chars[i - 1] == ' ' {
                    i -= 1;
                }
                let word = |ch: char| ch.is_alphanumeric() || ch == '_';
                if i > 0 && word(chars[i - 1]) {
                    while i > 0 && word(chars[i - 1]) {
                        i -= 1;
                    }
                } else {
                    i = i.saturating_sub(1);
                }
                let (a, b) = (byte_idx(&c.text, i), byte_idx(&c.text, c.cursor));
                c.text.replace_range(a..b, "");
                c.cursor = i;
            }
            KeyCode::Char('r') if key.ctrl => {
                c.awaiting_reg = true;
                return;
            }
            KeyCode::Char('v') if key.ctrl => {}
            KeyCode::Up | KeyCode::Down => {
                self.cmdline_history(key.code == KeyCode::Up);
                return;
            }
            KeyCode::Char('p') | KeyCode::Char('n') if key.ctrl => {
                self.cmdline_history(key.code == KeyCode::Char('p'));
                return;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                if c.kind == ':' {
                    self.cmdline_complete(key.code == KeyCode::Tab);
                }
                return;
            }
            KeyCode::Char(ch) if !key.ctrl && !key.alt => {
                let s = ch.to_string();
                self.cmdline_insert(&s);
                return;
            }
            _ => return,
        }
        self.cmdline.hist_idx = None;
        self.update_incsearch();
    }

    fn cmdline_insert(&mut self, s: &str) {
        let c = &mut self.cmdline;
        let b = byte_idx(&c.text, c.cursor);
        c.text.insert_str(b, s);
        c.cursor += s.chars().count();
        c.hist_idx = None;
        self.update_incsearch();
    }

    fn update_incsearch(&mut self) {
        let c = &self.cmdline;
        if c.kind == ':' || !self.options.incsearch {
            return;
        }
        let (origin, top) = (c.origin, c.origin_top);
        let fwd = c.kind == '/';
        let text = c.text.clone();
        self.win_mut().cursor = origin;
        self.win_mut().top = top;
        self.cmdline.inc_match = None;
        if text.is_empty() {
            return;
        }
        let Ok(re) = self.build_regex(&text, true) else { return };
        let from = self.buf().pos_to_char(origin);
        if let Some((s, e, _)) = self.find_match(&re, from, fwd, self.options.wrapscan) {
            let (ps, pe) = (self.buf().char_to_pos(s), self.buf().char_to_pos(e));
            self.cmdline.inc_match = Some((ps, pe));
            self.win_mut().cursor = ps;
            self.scroll_to_cursor();
            // Keep the real cursor at the origin; only the view follows the match.
            self.win_mut().cursor = origin;
        }
    }

    fn history(&mut self) -> &mut Vec<String> {
        if self.cmdline.kind == ':' { &mut self.cmd_history } else { &mut self.search_history }
    }

    fn cmdline_history(&mut self, up: bool) {
        let prefix = match self.cmdline.hist_idx {
            None => self.cmdline.text.clone(),
            Some(_) => self.cmdline.hist_prefix.clone(),
        };
        let idx = self.cmdline.hist_idx;
        let hist = self.history().clone();
        let mut i = idx.unwrap_or(hist.len()) as isize;
        loop {
            i += if up { -1 } else { 1 };
            if i < 0 {
                return;
            }
            if i as usize >= hist.len() {
                self.cmdline.text = prefix.clone();
                self.cmdline.hist_idx = None;
                break;
            }
            if hist[i as usize].starts_with(&prefix) {
                self.cmdline.text = hist[i as usize].clone();
                self.cmdline.hist_idx = Some(i as usize);
                self.cmdline.hist_prefix = prefix.clone();
                break;
            }
        }
        self.cmdline.cursor = self.cmdline.text.chars().count();
        self.update_incsearch();
    }

    fn cmdline_enter(&mut self) {
        let text = self.cmdline.text.clone();
        let kind = self.cmdline.kind;
        let pending = self.cmdline.pending_cmd.take();
        let (origin, top) = (self.cmdline.origin, self.cmdline.origin_top);
        self.cmdline.inc_match = None;
        self.mode = Mode::Normal;
        if !text.is_empty() {
            let h = self.history();
            h.retain(|x| *x != text);
            h.push(text.clone());
        }
        if kind == ':' {
            self.last_ex = Some(text.clone());
            self.registers.set_special(':', &text);
            self.batch_begin();
            let res = self.run_ex(&text);
            self.batch_end();
            if let Err(e) = res {
                self.error(e);
            }
            return;
        }
        self.win_mut().cursor = origin;
        self.win_mut().top = top;
        let pat = if text.is_empty() {
            match self.search.pattern.clone() {
                Some(p) => p,
                None => {
                    self.error("E35: No previous regular expression");
                    return;
                }
            }
        } else {
            text
        };
        let fwd = kind == '/';
        self.search.pattern = Some(pat.clone());
        self.search.forward = fwd;
        self.search.hl = true;
        self.registers.set_special('/', &pat);
        let m = Motion::Search(Some(pat), fwd);
        match pending {
            Some(mut cmd) => {
                if let Act::Op(op, Target::Motion(Motion::Search(_, _))) = cmd.act {
                    cmd.act = Act::Op(op, Target::Motion(m));
                }
                self.execute(cmd);
            }
            None => self.execute(Cmd { reg: None, count: None, act: Act::Move(m) }),
        }
    }

    fn cmdline_complete(&mut self, forward: bool) {
        if self.cmdline.completion.is_none() {
            let text: String = self.cmdline.text.chars().take(self.cmdline.cursor).collect();
            let (start, items) = self.completions(&text);
            if items.is_empty() {
                return;
            }
            // Remember the original token as the last entry to cycle back to.
            let orig: String = text.chars().skip(start).collect();
            let mut items = items;
            items.push(orig);
            let n = items.len();
            self.cmdline.completion = Some((items, if forward { n - 1 } else { 0 }, start));
        }
        let (items, idx, start) = self.cmdline.completion.clone().unwrap();
        let n = items.len();
        let idx = if forward { (idx + 1) % n } else { (idx + n - 1) % n };
        self.cmdline.completion = Some((items.clone(), idx, start));
        let head: String = self.cmdline.text.chars().take(start).collect();
        let tail: String = self.cmdline.text.chars().skip(self.cmdline.cursor).collect();
        self.cmdline.text = format!("{head}{}{tail}", items[idx]);
        self.cmdline.cursor = head.chars().count() + items[idx].chars().count();
    }

    /// Completion candidates for the command line up to the cursor:
    /// (char index where the completed token starts, candidates).
    pub fn completions(&self, text: &str) -> (usize, Vec<String>) {
        // Skip a leading range.
        let trimmed = text.trim_start_matches(|c: char| c.is_ascii_digit() || ",.$%'<>+-;".contains(c));
        let skip = text.chars().count() - trimmed.chars().count();
        if !trimmed.contains(' ') {
            let items: Vec<String> =
                EX_COMMANDS.iter().filter(|c| c.starts_with(trimmed)).map(|c| c.to_string()).collect();
            return (skip, items);
        }
        let cmd = trimmed.split_whitespace().next().unwrap_or("").trim_end_matches('!');
        let arg_start = text.rfind(' ').map(|b| text[..b + 1].chars().count()).unwrap_or(0);
        let arg: String = text.chars().skip(arg_start).collect();
        let items = match cmd {
            "e" | "ed" | "edi" | "edit" | "w" | "wr" | "wri" | "writ" | "write" | "sp" | "spl" | "split" | "vs"
            | "vsp" | "vsplit" | "r" | "re" | "rea" | "read" | "sav" | "saveas" | "tabe" | "tabedit" | "tabnew" | "cd"
            | "new" | "vnew" => complete_path(&arg, cmd == "cd"),
            "set" | "se" | "setlocal" | "setl" => {
                let (neg, a) = if let Some(r) = arg.strip_prefix("no") { ("no", r) } else { ("", arg.as_str()) };
                OPTION_NAMES
                    .iter()
                    .map(|(l, _)| *l)
                    .filter(|l| l.starts_with(a))
                    .map(|l| format!("{neg}{l}"))
                    .collect()
            }
            "colo" | "color" | "colors" | "colorsc" | "colorsch" | "colorsche" | "colorschem" | "colorscheme" => {
                crate::theme::available(crate::theme::user_dir().as_deref())
                    .into_iter()
                    .filter(|n| n.starts_with(arg.as_str()))
                    .collect()
            }
            "b" | "bu" | "buf" | "buffer" | "bd" | "bdelete" => self
                .buffers
                .values()
                .filter_map(|b| b.path.as_ref().map(|p| p.display().to_string()))
                .filter(|n| n.contains(&arg))
                .collect(),
            _ => vec![],
        };
        (arg_start, items)
    }
}

fn complete_path(arg: &str, dirs_only: bool) -> Vec<String> {
    let expanded = if let Some(rest) = arg.strip_prefix("~/") {
        dirs::home_dir().map(|h| format!("{}/{rest}", h.display())).unwrap_or_else(|| arg.to_string())
    } else {
        arg.to_string()
    };
    let (dir, prefix) = match expanded.rfind('/') {
        Some(i) => (expanded[..=i].to_string(), expanded[i + 1..].to_string()),
        None => (String::new(), expanded.clone()),
    };
    let shown_dir = match arg.rfind('/') {
        Some(i) => arg[..=i].to_string(),
        None => String::new(),
    };
    let Ok(rd) = std::fs::read_dir(if dir.is_empty() { "." } else { &dir }) else { return vec![] };
    let mut out: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with(&prefix) || (name.starts_with('.') && !prefix.starts_with('.')) {
                return None;
            }
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if dirs_only && !is_dir {
                return None;
            }
            Some(format!("{shown_dir}{name}{}", if is_dir { "/" } else { "" }))
        })
        .collect();
    out.sort();
    out
}

fn byte_idx(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map(|(b, _)| b).unwrap_or(s.len())
}
