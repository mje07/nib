//! Ex commands (`:w`, `:s`, `:g`, `:set`, ...).

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use regex::{Captures, RegexBuilder};

use super::keymap::MapMode;
use super::layout::{self, Dir};
use super::register::{RegKind, RegVal};
use super::search::translate;
use super::{Editor, Mode};
use crate::key::{Key, keys_to_notation, register_to_keys};
use crate::markdown::editing::toggle_task;
use crate::options::OptValue;
use crate::types::Pos;

type R = Result<(), String>;

/// (full name, minimum abbreviation length)
const CMDS: &[(&str, usize)] = &[
    ("buffer", 1),
    ("bNext", 2),
    ("bdelete", 2),
    ("bfirst", 2),
    ("blast", 2),
    ("bnext", 2),
    ("bprevious", 2),
    ("buffers", 7),
    ("bwipeout", 2),
    ("cd", 2),
    ("close", 3),
    ("copy", 2),
    ("delete", 1),
    ("display", 2),
    ("edit", 1),
    ("enew", 3),
    ("exit", 3),
    ("file", 1),
    ("files", 5),
    ("global", 1),
    ("help", 1),
    ("imap", 2),
    ("inoremap", 3),
    ("iunmap", 2),
    ("join", 1),
    ("jumps", 2),
    ("ls", 2),
    ("move", 1),
    ("mark", 2),
    ("marks", 5),
    ("map", 3),
    ("messages", 3),
    ("new", 3),
    ("nohlsearch", 3),
    ("normal", 4),
    ("nmap", 2),
    ("nnoremap", 2),
    ("noremap", 2),
    ("nunmap", 3),
    ("only", 2),
    ("omap", 2),
    ("onoremap", 3),
    ("ounmap", 2),
    ("print", 1),
    ("put", 2),
    ("preview", 4),
    ("pwd", 2),
    ("quit", 1),
    ("qall", 2),
    ("quitall", 5),
    ("read", 1),
    ("redo", 3),
    ("registers", 3),
    ("resize", 3),
    ("retab", 3),
    ("substitute", 1),
    ("set", 2),
    ("setlocal", 4),
    ("saveas", 3),
    ("sort", 3),
    ("split", 2),
    ("t", 1),
    ("tabnew", 6),
    ("tabedit", 4),
    ("tabnext", 4),
    ("tabprevious", 4),
    ("tabNext", 4),
    ("tabclose", 4),
    ("tabonly", 4),
    ("tabfirst", 6),
    ("tablast", 4),
    ("task", 4),
    ("undo", 1),
    ("update", 2),
    ("unmap", 3),
    ("vglobal", 1),
    ("vsplit", 2),
    ("vnew", 3),
    ("vmap", 2),
    ("vnoremap", 2),
    ("vunmap", 2),
    ("vertical", 4),
    ("write", 1),
    ("wq", 2),
    ("wall", 2),
    ("wqall", 3),
    ("xit", 1),
    ("xall", 2),
    ("xmap", 2),
    ("xnoremap", 2),
    ("yank", 1),
];

fn resolve(name: &str) -> Option<&'static str> {
    CMDS.iter().find(|(full, min)| name.len() >= *min && full.starts_with(name)).map(|(f, _)| *f)
}

struct Parsed {
    /// 1-based inclusive range; 0 means "before the first line".
    range: Option<(usize, usize)>,
    name: String,
    bang: bool,
    raw: String,
}

impl Parsed {
    fn args(&self) -> &str {
        self.raw.trim()
    }
}

pub const HELP: &str = "\
mdvim — vim-style editor with live markdown preview
  Modes: i a I A o O (insert)  v V Ctrl-v (visual)  R (replace)  : / ? (cmdline)
  Motions: h j k l w b e ge W B E 0 ^ $ g_ gg G f t F T ; , % { } ( ) ]] [[ H M L n N * #
  Operators: d c y > < gu gU g~ gq gw g? with motions or text objects (iw aw ip a( i\" it ...)
  Edits: x X D C s S Y p P J gJ r ~ . u Ctrl-r Ctrl-a Ctrl-x  Macros: q{r} @{r} @@
  Windows: Ctrl-w s/v/w/h/j/k/l/q/o/=/+/-/</>  Tabs: gt gT :tabnew
  Folds (markdown headings): za zc zo zR zM   Links: gx (open URL) gf (open file)
  Markdown: <leader>x toggles a task; Enter continues lists; Tab/S-Tab indent items
  :preview toggles live preview for the window; :set nopreview disables it globally
  Ex: :w :q :x :e :s :g :v :norm :sort :m :t :d :y :pu :j :> :< :! :r :set :map :ls :b :reg :marks";

impl Editor {
    pub fn batch_begin(&mut self) {
        self.batch += 1;
    }
    pub fn batch_end(&mut self) {
        self.batch = self.batch.saturating_sub(1);
        if self.batch == 0 {
            self.commit();
        }
    }

    pub fn run_ex(&mut self, input: &str) -> R {
        self.run_ex_mod(input, false)
    }

    fn run_ex_mod(&mut self, input: &str, vertical: bool) -> R {
        let input = input.trim_start_matches([' ', ':', '\t']);
        if input.is_empty() || input.starts_with('"') {
            return Ok(());
        }
        let p = self.parse_ex(input)?;
        if !self.buffers.contains_key(&self.win().buf) {
            return Ok(());
        }
        let was_visual_range = input.starts_with("'<,'>");
        let res = self.exec_ex(p, vertical);
        if was_visual_range && self.mode.is_visual() {
            self.exit_visual();
        }
        if !self.windows.is_empty() && !self.quit {
            self.clamp_cursor();
        }
        res
    }

    fn parse_ex(&mut self, input: &str) -> Result<Parsed, String> {
        let c: Vec<char> = input.chars().collect();
        let mut i = 0;
        let range = self.parse_range(&c, &mut i)?;
        while i < c.len() && (c[i] == ' ' || c[i] == ':') {
            i += 1;
        }
        let mut name = String::new();
        if i < c.len() && c[i].is_ascii_alphabetic() {
            while i < c.len() && c[i].is_ascii_alphabetic() {
                name.push(c[i]);
                i += 1;
            }
            // `:s` followed by a flag-like letter run (e.g. `:sg`) is not supported;
            // `:ka` sets mark a.
        } else if i < c.len() {
            let ch = c[i];
            if matches!(ch, '!' | '&' | '=' | '#' | '@') {
                name.push(ch);
                i += 1;
                if ch == '&' && i < c.len() && c[i] == '&' {
                    name.push('&');
                    i += 1;
                }
            } else if ch == '<' || ch == '>' {
                while i < c.len() && c[i] == ch {
                    name.push(ch);
                    i += 1;
                }
            }
        }
        let mut bang = false;
        if i < c.len() && c[i] == '!' && name != "!" {
            bang = true;
            i += 1;
        }
        let raw: String = c[i..].iter().collect();
        Ok(Parsed { range, name, bang, raw })
    }

    fn parse_range(&mut self, c: &[char], i: &mut usize) -> Result<Option<(usize, usize)>, String> {
        let skip = |i: &mut usize| {
            while *i < c.len() && c[*i] == ' ' {
                *i += 1;
            }
        };
        skip(i);
        let last = self.buf().line_count();
        if *i < c.len() && c[*i] == '%' {
            *i += 1;
            return Ok(Some((1, last)));
        }
        let mut cur = self.cursor().line + 1;
        let a = self.parse_addr(c, i, cur)?;
        skip(i);
        if *i < c.len() && (c[*i] == ',' || c[*i] == ';') {
            if c[*i] == ';'
                && let Some(a) = a
            {
                cur = a.max(1);
                self.set_cursor(Pos::new(cur - 1, 0));
            }
            *i += 1;
            let b = self.parse_addr(c, i, cur)?;
            let (a, b) = (a.unwrap_or(cur), b.unwrap_or(cur));
            if a > last || b > last {
                return Err("E16: Invalid range".into());
            }
            return Ok(Some(if a <= b { (a, b) } else { (b, a) }));
        }
        match a {
            Some(a) if a > last => Err("E16: Invalid range".into()),
            Some(a) => Ok(Some((a, a))),
            None => Ok(None),
        }
    }

    fn parse_addr(&mut self, c: &[char], i: &mut usize, cur: usize) -> Result<Option<usize>, String> {
        while *i < c.len() && c[*i] == ' ' {
            *i += 1;
        }
        let last = self.buf().line_count();
        let mut base: Option<usize> = None;
        if *i < c.len() {
            match c[*i] {
                d if d.is_ascii_digit() => {
                    let mut n = 0usize;
                    while *i < c.len() && c[*i].is_ascii_digit() {
                        n = n.saturating_mul(10) + c[*i].to_digit(10).unwrap() as usize;
                        *i += 1;
                    }
                    base = Some(n.min(last));
                }
                '.' => {
                    *i += 1;
                    base = Some(cur);
                }
                '$' => {
                    *i += 1;
                    base = Some(last);
                }
                '\'' => {
                    let m = *c.get(*i + 1).ok_or("E20: Mark not set")?;
                    *i += 2;
                    let (b, p) = self.get_mark(m).ok_or("E20: Mark not set")?;
                    if b != self.win().buf {
                        return Err("E20: Mark not set".into());
                    }
                    base = Some(p.line + 1);
                }
                d @ ('/' | '?') => {
                    *i += 1;
                    let mut pat = String::new();
                    while *i < c.len() && c[*i] != d {
                        if c[*i] == '\\' && *i + 1 < c.len() && c[*i + 1] == d {
                            *i += 1;
                        }
                        pat.push(c[*i]);
                        *i += 1;
                    }
                    if *i < c.len() {
                        *i += 1;
                    }
                    let pat = if pat.is_empty() { self.search.pattern.clone().ok_or("E35: No previous regular expression")? } else { pat };
                    self.search.pattern = Some(pat.clone());
                    let re = self.build_regex(&pat, true)?;
                    let n = self.buf().line_count();
                    let found = (1..=n)
                        .map(|k| if d == '/' { (cur - 1 + k) % n } else { (cur - 1 + n - k) % n })
                        .find(|&l| re.is_match(&self.buf().line(l)))
                        .ok_or_else(|| format!("E486: Pattern not found: {pat}"))?;
                    base = Some(found + 1);
                }
                _ => {}
            }
        }
        loop {
            while *i < c.len() && c[*i] == ' ' && matches!(c.get(*i + 1), Some('+') | Some('-')) {
                *i += 1;
            }
            if *i >= c.len() || !(c[*i] == '+' || c[*i] == '-') {
                break;
            }
            let neg = c[*i] == '-';
            *i += 1;
            let mut n = 0usize;
            let mut digits = false;
            while *i < c.len() && c[*i].is_ascii_digit() {
                n = n * 10 + c[*i].to_digit(10).unwrap() as usize;
                *i += 1;
                digits = true;
            }
            if !digits {
                n = 1;
            }
            let b = base.unwrap_or(cur);
            base = Some(if neg { b.saturating_sub(n) } else { (b + n).min(last) });
        }
        Ok(base)
    }

    /// 0-based inclusive line range, defaulting to the cursor line.
    fn lines(&self, p: &Parsed) -> (usize, usize) {
        match p.range {
            Some((a, b)) => (a.max(1) - 1, b.max(1) - 1),
            None => {
                let l = self.cursor().line;
                (l, l)
            }
        }
    }

    fn all_lines(&self, p: &Parsed) -> (usize, usize) {
        match p.range {
            Some(_) => self.lines(p),
            None => (0, self.buf().line_count() - 1),
        }
    }

    fn exec_ex(&mut self, p: Parsed, vertical: bool) -> R {
        if p.name.is_empty() {
            // `:N` — go to line.
            if let Some((_, b)) = p.range {
                self.push_jump();
                let l = b.max(1) - 1;
                let c = self.buf().first_non_blank(l);
                self.set_cursor_col(Pos::new(l, c));
                return Ok(());
            }
            return Err(format!("E492: Not an editor command: {}", p.raw));
        }
        let name = match p.name.as_str() {
            "!" | "&" | "&&" | "=" | "#" | "@" => p.name.clone(),
            n if n.starts_with('<') || n.starts_with('>') => n.to_string(),
            "k" => "mark".to_string(),
            n => match resolve(n) {
                Some(full) => full.to_string(),
                None => return Err(format!("E492: Not an editor command: {}{}", p.name, p.raw)),
            },
        };
        let args = p.args().to_string();
        match name.as_str() {
            "write" | "update" => {
                if name == "update" && !self.buf().is_modified() {
                    return Ok(());
                }
                if let Some(cmd) = args.strip_prefix('!') {
                    let (l1, l2) = self.all_lines(&p);
                    let input = (l1..=l2).map(|l| self.buf().line(l)).collect::<Vec<_>>().join("\n") + "\n";
                    let out = run_shell(cmd, Some(&input))?;
                    self.show_lines(out.lines().map(String::from).collect());
                    return Ok(());
                }
                self.write(&p, if args.is_empty() { None } else { Some(expand_path(&args)) })
            }
            "wq" | "xit" | "exit" => {
                if name == "wq" || self.buf().is_modified() || !args.is_empty() {
                    self.write(&p, if args.is_empty() { None } else { Some(expand_path(&args)) })?;
                }
                self.quit_window(p.bang)
            }
            "quit" => self.quit_window(p.bang),
            "close" => {
                let id = self.win_id();
                if layout::leaves(&self.tab().root).len() == 1 && self.tabs.len() == 1 {
                    return Err("E444: Cannot close last window".into());
                }
                self.close_window(id);
                Ok(())
            }
            "qall" | "quitall" => {
                if !p.bang
                    && let Some(b) = self.any_modified()
                {
                    return Err(format!("E37: No write since last change for buffer \"{}\" (add ! to override)", b.display_name()));
                }
                self.quit = true;
                Ok(())
            }
            "wall" | "wqall" | "xall" => {
                let ids: Vec<usize> = self.buffers.keys().copied().collect();
                for id in ids {
                    let b = self.buffers.get_mut(&id).unwrap();
                    if b.is_modified() {
                        if b.path.is_none() {
                            return Err("E141: No file name for buffer".into());
                        }
                        b.save(None).map_err(|e| e.to_string())?;
                    }
                }
                if name != "wall" {
                    self.quit = true;
                }
                Ok(())
            }
            "edit" => {
                if args.is_empty() {
                    if self.buf().path.is_none() {
                        return Err("E32: No file name".into());
                    }
                    if self.buf().is_modified() && !p.bang {
                        return Err("E37: No write since last change (add ! to override)".into());
                    }
                    let cur = self.cursor();
                    self.buf_mut().reload().map_err(|e| e.to_string())?;
                    self.set_cursor(cur);
                    let info = self.file_info();
                    self.info(info);
                    return Ok(());
                }
                self.edit_file(&args)
            }
            "enew" => {
                let b = self.new_buffer();
                self.switch_buffer(b);
                Ok(())
            }
            "read" => {
                let (_, l2) = self.lines(&p);
                let after = match p.range {
                    Some((0, _)) => None,
                    _ => Some(l2),
                };
                let text = if let Some(cmd) = args.strip_prefix('!') {
                    run_shell(cmd, None)?
                } else {
                    let path = if args.is_empty() {
                        self.buf().path.clone().ok_or("E32: No file name")?
                    } else {
                        expand_path(&args)
                    };
                    std::fs::read_to_string(&path).map_err(|e| format!("E484: Can't open file {}: {e}", path.display()))?
                };
                let text = text.strip_suffix('\n').unwrap_or(&text).to_string();
                self.begin_change();
                let line = match after {
                    Some(l) => {
                        let e = self.buf().line_end(l);
                        self.ins(e, &format!("\n{text}"));
                        l + 1
                    }
                    None => {
                        self.ins(0, &format!("{text}\n"));
                        0
                    }
                };
                let c = self.buf().first_non_blank(line);
                self.set_cursor_col(Pos::new(line, c));
                Ok(())
            }
            "saveas" => {
                if args.is_empty() {
                    return Err("E471: Argument required".into());
                }
                let path = expand_path(&args);
                self.buf_mut().path = Some(path);
                let ft = crate::buffer::FileType::detect(self.buf().path.as_deref());
                self.buf_mut().filetype = ft;
                self.write(&Parsed { range: None, name: String::new(), bang: false, raw: String::new() }, None)
            }
            "file" => {
                if !args.is_empty() {
                    let path = expand_path(&args);
                    let ft = crate::buffer::FileType::detect(Some(&path));
                    let b = self.buf_mut();
                    b.path = Some(path);
                    b.filetype = ft;
                }
                let s = self.file_info();
                self.info(s);
                Ok(())
            }
            "substitute" | "&" | "&&" => {
                let (l1, l2) = self.lines(&p);
                let args = if name == "&&" { format!("&{}", p.raw.trim()) } else { p.raw.trim_start().to_string() };
                let args = if name == "&" || name == "&&" { args } else { p.raw.clone() };
                self.substitute(l1, l2, &args)
            }
            "global" | "vglobal" => {
                let (l1, l2) = self.all_lines(&p);
                self.global(l1, l2, p.raw.trim_start(), p.bang || name == "vglobal")
            }
            "delete" | "yank" => {
                let (mut l1, mut l2) = self.lines(&p);
                let mut reg = None;
                let mut rest = args.as_str();
                if let Some(c) = rest.chars().next()
                    && !c.is_ascii_digit()
                {
                    reg = Some(c);
                    rest = rest[c.len_utf8()..].trim();
                }
                if let Ok(n) = rest.parse::<usize>() {
                    l1 = l2;
                    l2 = (l1 + n - 1).min(self.buf().line_count() - 1);
                }
                let op = if name == "delete" { super::normal::Op::Delete } else { super::normal::Op::Yank };
                self.begin_change();
                self.apply_op(op, super::operator::ORange::Lines(l1, l2), reg, 1);
                Ok(())
            }
            "put" => {
                let (_, l) = self.lines(&p);
                let reg = args.chars().next().unwrap_or('"');
                let val = self.registers.get(reg).ok_or_else(|| format!("E353: Nothing in register {reg}"))?;
                let val = RegVal::new(val.text, RegKind::Line);
                self.begin_change();
                match p.range {
                    Some((0, _)) => {
                        self.set_cursor(Pos::new(0, 0));
                        self.put_val(&val, true, false, 1);
                    }
                    _ => {
                        self.set_cursor(Pos::new(l, 0));
                        self.put_val(&val, p.bang, false, 1);
                    }
                }
                Ok(())
            }
            "move" | "copy" | "t" => {
                let (l1, l2) = self.lines(&p);
                let c: Vec<char> = args.chars().collect();
                let mut i = 0;
                let cur = self.cursor().line + 1;
                let dest = self.parse_addr(&c, &mut i, cur)?.ok_or("E14: Invalid address")?;
                self.begin_change();
                self.move_copy(l1, l2, dest, name == "move")
            }
            "join" => {
                let (l1, mut l2) = self.lines(&p);
                if let Ok(n) = args.parse::<usize>() {
                    l2 = l2 + n - 1;
                }
                let joins = if l2 > l1 { l2 - l1 } else { 1 };
                self.begin_change();
                self.join_lines(l1, joins, !p.bang);
                Ok(())
            }
            n if n.starts_with('<') || n.starts_with('>') => {
                let (mut l1, mut l2) = self.lines(&p);
                if let Ok(cnt) = args.parse::<usize>() {
                    l1 = l2;
                    l2 = (l1 + cnt - 1).min(self.buf().line_count() - 1);
                }
                self.begin_change();
                self.shift_lines(l1, l2, n.starts_with('>'), n.len());
                let c = self.buf().first_non_blank(l2);
                self.set_cursor_col(Pos::new(l2, c));
                Ok(())
            }
            "normal" => {
                let keys: Vec<Key> = register_to_keys(&p.raw.trim_start().replace('\n', "\r"));
                self.normal_cmd(p.range.map(|_| self.lines(&p)), &keys, !p.bang);
                Ok(())
            }
            "sort" => {
                let (l1, l2) = self.all_lines(&p);
                self.begin_change();
                self.sort_lines(l1, l2, &args, p.bang);
                Ok(())
            }
            "undo" => {
                self.execute(super::normal::Cmd { reg: None, count: None, act: super::normal::Act::Undo });
                Ok(())
            }
            "redo" => {
                self.execute(super::normal::Cmd { reg: None, count: None, act: super::normal::Act::Redo });
                Ok(())
            }
            "set" | "setlocal" => self.set_options(&args, name == "setlocal"),
            "map" | "noremap" | "nmap" | "nnoremap" | "vmap" | "vnoremap" | "xmap" | "xnoremap" | "imap" | "inoremap"
            | "omap" | "onoremap" => {
                let modes: Vec<MapMode> = match name.chars().next().unwrap() {
                    'n' if name.starts_with("nn") || name == "nmap" => vec![MapMode::Normal],
                    'v' | 'x' => vec![MapMode::Visual],
                    'i' => vec![MapMode::Insert],
                    'o' => vec![MapMode::Operator],
                    _ => vec![MapMode::Normal, MapMode::Visual, MapMode::Operator],
                };
                let modes = if p.bang && (name == "map" || name == "noremap") { vec![MapMode::Insert] } else { modes };
                let noremap = name.contains("noremap");
                match args.split_once(char::is_whitespace) {
                    Some((lhs, rhs)) => {
                        let leader = self.leader.clone();
                        for m in modes {
                            self.keymaps.add(m, lhs, rhs.trim_start(), noremap, &leader);
                        }
                        Ok(())
                    }
                    None => {
                        let mut lines = self.keymaps.list(&modes);
                        if !args.is_empty() {
                            lines.retain(|l| l[3..].starts_with(&args));
                        }
                        if lines.is_empty() {
                            self.info("No mapping found");
                        } else {
                            self.show_lines(lines);
                        }
                        Ok(())
                    }
                }
            }
            "unmap" | "nunmap" | "vunmap" | "iunmap" | "ounmap" => {
                let modes: Vec<MapMode> = match name.as_str() {
                    "nunmap" => vec![MapMode::Normal],
                    "vunmap" => vec![MapMode::Visual],
                    "iunmap" => vec![MapMode::Insert],
                    "ounmap" => vec![MapMode::Operator],
                    _ => vec![MapMode::Normal, MapMode::Visual, MapMode::Operator],
                };
                let leader = self.leader.clone();
                let mut any = false;
                for m in modes {
                    any |= self.keymaps.remove(m, &args, &leader);
                }
                if any { Ok(()) } else { Err("E31: No such mapping".into()) }
            }
            "split" | "new" | "vsplit" | "vnew" => {
                let dir = if vertical || name.starts_with('v') { Dir::Vertical } else { Dir::Horizontal };
                let buf = if name.ends_with("new") {
                    Some(self.new_buffer())
                } else if !args.is_empty() {
                    Some(self.buffer_for_path(&expand_path(&args))?)
                } else {
                    None
                };
                self.split(dir, buf);
                Ok(())
            }
            "only" => {
                self.only_window();
                Ok(())
            }
            "tabnew" | "tabedit" => {
                let buf = if args.is_empty() { self.new_buffer() } else { self.buffer_for_path(&expand_path(&args))? };
                self.new_tab(buf);
                Ok(())
            }
            "tabnext" => {
                let n = args.parse::<usize>().ok().or(p.range.map(|r| r.1));
                match n {
                    Some(n) => self.goto_tab(n.saturating_sub(1)),
                    None => self.goto_tab((self.tab + 1) % self.tabs.len()),
                }
                Ok(())
            }
            "tabprevious" | "tabNext" => {
                let n = args.parse::<usize>().unwrap_or(1) % self.tabs.len();
                let len = self.tabs.len();
                self.goto_tab((self.tab + len - n) % len);
                Ok(())
            }
            "tabfirst" => {
                self.goto_tab(0);
                Ok(())
            }
            "tablast" => {
                self.goto_tab(self.tabs.len() - 1);
                Ok(())
            }
            "tabclose" => {
                if self.tabs.len() == 1 {
                    return Err("E784: Cannot close last tab page".into());
                }
                let idx = args.parse::<usize>().map(|n| n - 1).unwrap_or(self.tab);
                self.close_tab(idx.min(self.tabs.len() - 1));
                Ok(())
            }
            "tabonly" => {
                let keep = self.tab;
                for i in (0..self.tabs.len()).rev() {
                    if i != keep {
                        self.close_tab(i);
                    }
                }
                self.tab = 0;
                Ok(())
            }
            "buffers" | "files" | "ls" => {
                let cur = self.win().buf;
                let alt = self.win().alt_buf;
                let shown: Vec<usize> = self.windows.values().map(|w| w.buf).collect();
                let lines = self
                    .buffers
                    .values()
                    .map(|b| {
                        let flag = if b.id == cur { '%' } else if Some(b.id) == alt { '#' } else { ' ' };
                        let act = if shown.contains(&b.id) { 'a' } else { 'h' };
                        let line = if b.id == cur { self.cursor().line + 1 } else { b.last_cursor.line + 1 };
                        format!(
                            "{:>3} {}{} {} {:<30} line {}",
                            b.id,
                            flag,
                            act,
                            if b.is_modified() { '+' } else { ' ' },
                            format!("\"{}\"", b.display_name()),
                            line
                        )
                    })
                    .collect();
                self.show_lines(lines);
                Ok(())
            }
            "buffer" => {
                if args.is_empty() {
                    return match p.range {
                        Some((_, n)) if self.buffers.contains_key(&n) => {
                            self.switch_buffer(n);
                            Ok(())
                        }
                        Some((_, n)) => Err(format!("E86: Buffer {n} does not exist")),
                        None => Ok(()),
                    };
                }
                let id = self.find_buffer(&args)?;
                self.switch_buffer(id);
                Ok(())
            }
            "bnext" | "bprevious" | "bNext" | "bfirst" | "blast" => {
                let ids: Vec<usize> = self.buffers.keys().copied().collect();
                let cur = ids.iter().position(|b| *b == self.win().buf).unwrap_or(0);
                let n = args.parse::<usize>().unwrap_or(1);
                let target = match name.as_str() {
                    "bnext" => ids[(cur + n) % ids.len()],
                    "bfirst" => ids[0],
                    "blast" => *ids.last().unwrap(),
                    _ => ids[(cur + ids.len() * n - n) % ids.len()],
                };
                self.switch_buffer(target);
                Ok(())
            }
            "bdelete" | "bwipeout" => {
                let id = if args.is_empty() {
                    match p.range {
                        Some((_, n)) => n,
                        None => self.win().buf,
                    }
                } else {
                    self.find_buffer(&args)?
                };
                self.delete_buffer(id, p.bang)
            }
            "registers" | "display" => {
                let mut lines = vec!["Type Name Content".to_string()];
                for (c, v) in self.registers.list() {
                    if !args.is_empty() && !args.contains(c) {
                        continue;
                    }
                    let t = match v.kind {
                        RegKind::Char => 'c',
                        RegKind::Line => 'l',
                        RegKind::Block => 'b',
                    };
                    let mut content = keys_to_notation(&register_to_keys(&v.text)).replace("<CR>", "^J");
                    if v.kind == RegKind::Line {
                        content.push_str("^J");
                    }
                    let content: String = content.chars().take(self.size.0.saturating_sub(12) as usize).collect();
                    lines.push(format!("  {t}  \"{c}   {content}"));
                }
                self.show_lines(lines);
                Ok(())
            }
            "marks" => {
                let mut lines = vec!["mark line  col file/text".to_string()];
                let mut marks: Vec<(char, Pos)> = self.buf().marks.iter().map(|(c, p)| (*c, *p)).collect();
                marks.sort();
                for (c, p) in marks {
                    let text = self.buf().line(p.line.min(self.buf().line_count() - 1));
                    lines.push(format!(" {c} {:>6} {:>4} {}", p.line + 1, p.col, text.trim()));
                }
                let mut g: Vec<_> = self.global_marks.iter().collect();
                g.sort();
                for (c, (b, p)) in g {
                    let name = self.buffers.get(b).map(|b| b.display_name()).unwrap_or_default();
                    lines.push(format!(" {c} {:>6} {:>4} {}", p.line + 1, p.col, name));
                }
                self.show_lines(lines);
                Ok(())
            }
            "jumps" => {
                let w = self.win();
                let mut lines = vec![" jump line  col file/text".to_string()];
                for (i, (b, p)) in w.jumps.iter().enumerate() {
                    let rel = (i as isize - w.jump_idx as isize).unsigned_abs();
                    let name = self.buffers.get(b).map(|b| b.display_name()).unwrap_or_default();
                    lines.push(format!("{:>4} {:>6} {:>4} {}", rel, p.line + 1, p.col, name));
                }
                lines.push(">".into());
                self.show_lines(lines);
                Ok(())
            }
            "nohlsearch" => {
                self.search.hl = false;
                Ok(())
            }
            "preview" => {
                let on = match args.as_str() {
                    "on" => true,
                    "off" => false,
                    _ => !self.preview_on(self.win()),
                };
                if !self.buf().is_markdown() {
                    return Err("Live preview only applies to markdown buffers (:set ft=markdown)".into());
                }
                self.win_mut().preview = Some(on);
                self.info(if on { "Live preview on" } else { "Live preview off (source mode)" });
                Ok(())
            }
            "task" => {
                let (l1, l2) = self.lines(&p);
                self.begin_change();
                for l in l1..=l2 {
                    let line = self.buf().line(l);
                    if line.trim().is_empty() && l1 != l2 {
                        continue;
                    }
                    let new = toggle_task(&line);
                    self.set_line(l, &new);
                }
                let c = self.cursor().col;
                self.set_cursor(Pos::new(l1, c));
                Ok(())
            }
            "pwd" => {
                let d = std::env::current_dir().map_err(|e| e.to_string())?;
                self.info(d.display().to_string());
                Ok(())
            }
            "cd" => {
                let dir = if args.is_empty() { dirs::home_dir().unwrap_or_default() } else { expand_path(&args) };
                std::env::set_current_dir(&dir).map_err(|e| format!("E344: Can't find directory \"{}\": {e}", dir.display()))?;
                self.info(dir.display().to_string());
                Ok(())
            }
            "help" => {
                self.show_lines(HELP.lines().map(String::from).collect());
                Ok(())
            }
            "messages" => Ok(()),
            "mark" => {
                let c = args.chars().next().ok_or("E471: Argument required")?;
                let (_, l) = self.lines(&p);
                let saved = self.cursor();
                self.win_mut().cursor = Pos::new(l, 0);
                let ok = self.set_mark(c);
                self.win_mut().cursor = saved;
                if ok { Ok(()) } else { Err("E191: Argument must be a letter or forward/backward quote".into()) }
            }
            "=" => {
                let n = match p.range {
                    Some((_, b)) => b,
                    None => self.buf().line_count(),
                };
                self.info(n.to_string());
                Ok(())
            }
            "print" | "#" => {
                let (l1, l2) = self.lines(&p);
                let lines = (l1..=l2)
                    .map(|l| if name == "#" { format!("{:>3} {}", l + 1, self.buf().line(l)) } else { self.buf().line(l) })
                    .collect();
                self.show_lines(lines);
                Ok(())
            }
            "!" => {
                let cmd = p.raw.trim();
                if cmd.is_empty() {
                    return Err("E471: Argument required".into());
                }
                if p.range.is_some() {
                    let (l1, l2) = self.lines(&p);
                    let input = (l1..=l2).map(|l| self.buf().line(l)).collect::<Vec<_>>().join("\n") + "\n";
                    let out = run_shell(cmd, Some(&input))?;
                    let out = out.strip_suffix('\n').unwrap_or(&out).to_string();
                    let (a, e) = (self.buf().line_start(l1), self.buf().line_end(l2));
                    self.begin_change();
                    self.del(a, e);
                    self.ins(a, &out);
                    self.set_cursor_col(Pos::new(l1, 0));
                    let n = l2 - l1 + 1;
                    self.info(format!("{n} lines filtered"));
                } else {
                    let out = run_shell(cmd, None)?;
                    let mut lines: Vec<String> = vec![format!(":!{cmd}")];
                    lines.extend(out.lines().map(String::from));
                    lines.push(String::new());
                    self.show_lines(lines);
                    self.hit_enter = true;
                }
                Ok(())
            }
            "resize" => {
                let id = self.win_id();
                let area = self.editor_area();
                let dir = if vertical { Dir::Vertical } else { Dir::Horizontal };
                let r = self.window_rect(id);
                let cur = if vertical { r.w as i32 } else { r.h as i32 };
                let delta = if let Some(n) = args.strip_prefix('+') {
                    n.parse::<i32>().unwrap_or(1)
                } else if let Some(n) = args.strip_prefix('-') {
                    -n.parse::<i32>().unwrap_or(1)
                } else if let Ok(n) = args.parse::<i32>() {
                    n - cur
                } else {
                    1000
                };
                layout::resize(&mut self.tab_mut().root, area, id, dir, delta);
                Ok(())
            }
            "vertical" => self.run_ex_mod(&p.raw, true),
            "retab" => {
                let (l1, l2) = self.all_lines(&p);
                let ts = self.options.tabstop;
                self.begin_change();
                for l in l1..=l2 {
                    let line = self.buf().line(l);
                    if !line.contains('\t') {
                        continue;
                    }
                    let mut out = String::new();
                    let mut col = 0;
                    for ch in line.chars() {
                        if ch == '\t' {
                            let n = ts - col % ts;
                            out.push_str(&" ".repeat(n));
                            col += n;
                        } else {
                            out.push(ch);
                            col += 1;
                        }
                    }
                    self.set_line(l, &out);
                }
                Ok(())
            }
            "@" => {
                let r = args.chars().next().unwrap_or('@');
                let (_, l) = self.lines(&p);
                self.set_cursor(Pos::new(l, 0));
                let keys = if r == '@' { vec![Key::ch('@'), Key::ch('@')] } else { vec![Key::ch('@'), Key::ch(r)] };
                self.exec_keys(&keys, true);
                Ok(())
            }
            _ => Err(format!("E492: Not an editor command: {}", p.name)),
        }
    }

    fn write(&mut self, p: &Parsed, path: Option<PathBuf>) -> R {
        if p.range.is_some()
            && let Some(path) = &path
        {
            let (l1, l2) = self.lines(p);
            let text = (l1..=l2).map(|l| self.buf().line(l)).collect::<Vec<_>>().join("\n") + "\n";
            std::fs::write(path, &text).map_err(|e| format!("E212: Can't open file for writing: {e}"))?;
            self.info(format!("\"{}\" {}L, {}B written", path.display(), l2 - l1 + 1, text.len()));
            return Ok(());
        }
        if let Some(pp) = &path
            && pp.exists()
            && !p.bang
            && self.buf().path.as_ref().map(|x| super::canonical(x)) != Some(super::canonical(pp))
        {
            return Err("E13: File exists (add ! to override)".into());
        }
        self.commit();
        let b = self.buf_mut();
        let name = path.clone().or_else(|| b.path.clone()).map(|p| p.display().to_string()).unwrap_or_default();
        match b.save(path.as_deref()) {
            Ok((lines, bytes)) => {
                self.info(format!("\"{name}\" {lines}L, {bytes}B written"));
                Ok(())
            }
            Err(e) => Err(format!("E212: Can't open file for writing: {e}")),
        }
    }

    pub fn quit_window(&mut self, force: bool) -> R {
        let id = self.win_id();
        let last = layout::leaves(&self.tab().root).len() == 1 && self.tabs.len() == 1;
        if last {
            if !force {
                let cur = self.win().buf;
                if self.buf().is_modified() {
                    return Err("E37: No write since last change (add ! to override)".into());
                }
                if let Some(b) = self.buffers.values().find(|b| b.is_modified() && b.id != cur) {
                    return Err(format!("E162: No write since last change for buffer \"{}\"", b.display_name()));
                }
            }
            self.quit = true;
            return Ok(());
        }
        self.close_window(id);
        Ok(())
    }

    fn edit_file(&mut self, arg: &str) -> R {
        let path = expand_path(arg);
        let id = self.buffer_for_path(&path)?;
        self.push_jump();
        self.switch_buffer(id);
        Ok(())
    }

    fn find_buffer(&self, arg: &str) -> Result<usize, String> {
        if let Ok(n) = arg.parse::<usize>() {
            return if self.buffers.contains_key(&n) { Ok(n) } else { Err(format!("E86: Buffer {n} does not exist")) };
        }
        let matches: Vec<usize> =
            self.buffers.values().filter(|b| b.display_name().contains(arg)).map(|b| b.id).collect();
        match matches.len() {
            0 => Err(format!("E94: No matching buffer for {arg}")),
            1 => Ok(matches[0]),
            _ => {
                // Prefer an exact file-name match.
                self.buffers
                    .values()
                    .find(|b| b.path.as_ref().and_then(|p| p.file_name()).is_some_and(|f| f.to_string_lossy() == arg))
                    .map(|b| b.id)
                    .ok_or_else(|| format!("E93: More than one match for {arg}"))
            }
        }
    }

    fn delete_buffer(&mut self, id: usize, force: bool) -> R {
        let b = self.buffers.get(&id).ok_or_else(|| format!("E516: No buffers were deleted: {id}"))?;
        if b.is_modified() && !force {
            return Err(format!("E89: No write since last change for buffer {id} (add ! to override)"));
        }
        let replacement = self
            .buffers
            .keys()
            .copied()
            .rfind(|b| *b != id)
            .unwrap_or_else(|| self.new_buffer());
        let wins: Vec<usize> = self.windows.iter().filter(|(_, w)| w.buf == id).map(|(k, _)| *k).collect();
        let cur = self.win_id();
        for w in wins {
            let alt = self.windows[&w].alt_buf.filter(|a| *a != id && self.buffers.contains_key(a));
            let target = alt.unwrap_or(replacement);
            let win = self.windows.get_mut(&w).unwrap();
            win.buf = target;
            win.cursor = self.buffers[&target].last_cursor;
            win.top = 0;
        }
        self.buffers.remove(&id);
        for w in self.windows.values_mut() {
            if w.alt_buf == Some(id) {
                w.alt_buf = None;
            }
            w.jumps.retain(|(b, _)| *b != id);
            w.jump_idx = w.jump_idx.min(w.jumps.len());
        }
        self.global_marks.retain(|_, (b, _)| *b != id);
        let _ = cur;
        self.clamp_cursor();
        Ok(())
    }

    fn move_copy(&mut self, l1: usize, l2: usize, dest: usize, is_move: bool) -> R {
        // `dest` is 1-based: insert after line `dest` (0 = top).
        if is_move && dest > l1 && dest <= l2 {
            return Err("E134: Cannot move a range of lines into itself".into());
        }
        let text: Vec<String> = (l1..=l2).map(|l| self.buf().line(l)).collect();
        let n = text.len();
        let joined = text.join("\n");
        let mut dest = dest;
        if is_move {
            if dest == l1 && dest <= l2 {
                // Moving after the line just before: no-op.
                return Ok(());
            }
            self.delete_lines(l1, l2);
            if dest > l2 {
                dest -= n;
            }
        }
        if dest == 0 {
            self.ins(0, &format!("{joined}\n"));
        } else if dest >= self.buf().line_count() {
            let total = self.buf().len_chars();
            self.ins(total, &format!("\n{joined}"));
        } else {
            let st = self.buf().line_start(dest);
            self.ins(st, &format!("{joined}\n"));
        }
        let last = dest + n - 1;
        let c = self.buf().first_non_blank(last);
        self.set_cursor_col(Pos::new(last, c));
        Ok(())
    }

    fn sort_lines(&mut self, l1: usize, l2: usize, flags: &str, reverse: bool) {
        let mut lines: Vec<String> = (l1..=l2).map(|l| self.buf().line(l)).collect();
        let numeric = flags.contains('n');
        let icase = flags.contains('i');
        let unique = flags.contains('u');
        if numeric {
            let re = regex::Regex::new(r"-?\d+").unwrap();
            let key = |s: &String| re.find(s).and_then(|m| m.as_str().parse::<i64>().ok());
            lines.sort_by_key(key);
        } else if icase {
            lines.sort_by_key(|a| a.to_lowercase());
        } else {
            lines.sort();
        }
        if reverse {
            lines.reverse();
        }
        if unique {
            lines.dedup_by(|a, b| if icase { a.eq_ignore_ascii_case(b) } else { a == b });
        }
        let (a, e) = (self.buf().line_start(l1), self.buf().line_end(l2));
        let new = lines.join("\n");
        if self.buf().slice(a, e) != new {
            self.del(a, e);
            self.ins(a, &new);
        }
        self.set_cursor_col(Pos::new(l1, 0));
    }

    fn normal_cmd(&mut self, range: Option<(usize, usize)>, keys: &[Key], remap: bool) {
        let run = |ed: &mut Editor| {
            ed.exec_keys(keys, remap);
            // :normal ends any unfinished command or mode.
            ed.pending.clear();
            if ed.mode.is_insert() {
                ed.exec_keys(&[Key::ESC], false);
            }
            if ed.mode.is_visual() {
                ed.exit_visual();
            }
            if ed.mode == Mode::Cmdline {
                ed.exec_keys(&[Key::ESC], false);
            }
        };
        match range {
            None => run(self),
            Some((l1, l2)) => {
                let mut targets: Vec<usize> = (l1..=l2).collect();
                let mut i = 0;
                while i < targets.len() {
                    let l = targets[i];
                    if l >= self.buf().line_count() {
                        break;
                    }
                    let before = self.buf().line_count() as isize;
                    self.win_mut().cursor = Pos::new(l, 0);
                    run(self);
                    let delta = self.buf().line_count() as isize - before;
                    for t in targets.iter_mut().skip(i + 1) {
                        *t = (*t as isize + delta).max(0) as usize;
                    }
                    i += 1;
                }
            }
        }
    }

    fn set_options(&mut self, args: &str, local: bool) -> R {
        if args.is_empty() || args == "all" {
            let mut lines = Vec::new();
            for (name, _) in crate::options::OPTION_NAMES {
                match self.options.get(name) {
                    Some(OptValue::Bool(b)) => lines.push(format!("  {}{}", if b { "" } else { "no" }, name)),
                    Some(OptValue::Num(n)) => lines.push(format!("  {name}={n}")),
                    None => {}
                }
            }
            lines.push(format!("  filetype={}", self.buf().filetype.name()));
            self.show_lines(lines);
            return Ok(());
        }
        for arg in args.split_whitespace() {
            if let Some(v) = arg.strip_prefix("filetype=").or_else(|| arg.strip_prefix("ft=")) {
                self.buf_mut().filetype = match v {
                    "markdown" | "md" => crate::buffer::FileType::Markdown,
                    _ => crate::buffer::FileType::Text,
                };
                continue;
            }
            if arg == "filetype?" || arg == "ft?" || arg == "filetype" || arg == "ft" {
                self.info(format!("  filetype={}", self.buf().filetype.name()));
                continue;
            }
            if let Some((name, val)) = arg.split_once("+=").or_else(|| arg.split_once("-=")).or_else(|| arg.split_once('=')) {
                let n: usize = val.parse().map_err(|_| format!("E521: Number required after =: {arg}"))?;
                let cur = match self.options.get(name) {
                    Some(OptValue::Num(c)) => c,
                    Some(OptValue::Bool(_)) => return Err(format!("E474: Invalid argument: {arg}")),
                    None => return Err(format!("E518: Unknown option: {name}")),
                };
                let v = if arg.contains("+=") {
                    cur + n
                } else if arg.contains("-=") {
                    cur.saturating_sub(n)
                } else {
                    n
                };
                self.options.set_num(name, v)?;
                continue;
            }
            let (name, query, toggle) = if let Some(n) = arg.strip_suffix('?') {
                (n, true, false)
            } else if let Some(n) = arg.strip_suffix('!') {
                (n, false, true)
            } else if let Some(n) = arg.strip_prefix("inv") {
                (n, false, true)
            } else {
                (arg, false, false)
            };
            if query || (!self.options.is_bool(name) && self.options.get(name).is_some() && !toggle) {
                match self.options.get(name) {
                    Some(OptValue::Bool(b)) => self.info(format!("  {}{}", if b { "" } else { "no" }, name)),
                    Some(OptValue::Num(n)) => self.info(format!("  {name}={n}")),
                    None => return Err(format!("E518: Unknown option: {name}")),
                }
                continue;
            }
            let (name, value) = match name.strip_prefix("no") {
                Some(rest) if self.options.is_bool(rest) => (rest, false),
                _ => (name, true),
            };
            let value = if toggle {
                match self.options.get(name) {
                    Some(OptValue::Bool(b)) => !b,
                    _ => return Err(format!("E488: Trailing characters: {arg}")),
                }
            } else {
                value
            };
            if local && crate::options::canonical(name) == Some("preview") {
                self.win_mut().preview = Some(value);
                continue;
            }
            self.options.set_bool(name, value)?;
            if crate::options::canonical(name) == Some("preview") {
                for w in self.windows.values_mut() {
                    w.preview = None;
                }
            }
            if crate::options::canonical(name) == Some("hlsearch") && value {
                self.search.hl = true;
            }
            if crate::options::canonical(name) == Some("clipboard") {
                self.registers.use_clipboard = value;
            }
        }
        self.scroll_to_cursor();
        Ok(())
    }

    fn substitute(&mut self, l1: usize, l2: usize, args: &str) -> R {
        let chars: Vec<char> = args.chars().collect();
        let delim_ok = |c: char| !(c.is_alphanumeric() || c.is_whitespace() || c == '\\' || c == '"' || c == '|');
        let (pat, rep, flags_str) = if !chars.is_empty() && delim_ok(chars[0]) && chars[0] != '&' {
            let d = chars[0];
            let mut i = 1;
            let mut parts = [String::new(), String::new()];
            for part in parts.iter_mut() {
                while i < chars.len() && chars[i] != d {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        if chars[i + 1] == d {
                            part.push(d);
                        } else {
                            part.push('\\');
                            part.push(chars[i + 1]);
                        }
                        i += 2;
                        continue;
                    }
                    part.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
            }
            let flags: String = chars[i.min(chars.len())..].iter().collect();
            (parts[0].clone(), parts[1].clone(), flags)
        } else {
            let (p, r, f) = self.last_sub.clone().ok_or("E35: No previous regular expression")?;
            let mut flags: String = args.trim().to_string();
            if let Some(rest) = flags.strip_prefix('&') {
                flags = format!("{f}{rest}");
            }
            (p, r, flags)
        };
        let pat = if pat.is_empty() {
            self.search.pattern.clone().or_else(|| self.last_sub.as_ref().map(|s| s.0.clone())).ok_or("E35: No previous regular expression")?
        } else {
            pat
        };
        let flags: String = flags_str.trim().chars().take_while(|c| c.is_alphabetic() || *c == '&').collect();
        let count_arg: Option<usize> = flags_str.trim()[flags.len()..].trim().parse().ok();
        let (l1, l2) = match count_arg {
            Some(n) => (l2, (l2 + n - 1).min(self.buf().line_count() - 1)),
            None => (l1, l2),
        };
        self.last_sub = Some((pat.clone(), rep.clone(), flags.clone()));
        self.search.pattern = Some(pat.clone());
        self.search.hl = true;
        let global = flags.contains('g');
        let count_only = flags.contains('n');
        let (tp, case) = translate(&pat);
        let ignore = if flags.contains('I') {
            false
        } else if flags.contains('i') {
            true
        } else {
            case.unwrap_or(self.options.ignorecase && !(self.options.smartcase && tp.chars().any(|c| c.is_uppercase())))
        };
        let re = RegexBuilder::new(&tp).case_insensitive(ignore).build().map_err(|e| format!("E486: {e}"))?;
        let toks = parse_rep(&rep);
        let mut total = 0;
        let mut lines_changed = 0;
        let mut last_line = None;
        let mut l = l1;
        let mut end = l2;
        self.begin_change();
        while l <= end && l < self.buf().line_count() {
            let text = self.buf().line(l);
            let mut new = String::new();
            let mut last = 0;
            let mut n = 0;
            for caps in re.captures_iter(&text) {
                let m = caps.get(0).unwrap();
                new.push_str(&text[last..m.start()]);
                new.push_str(&expand_rep(&toks, &caps));
                last = m.end();
                n += 1;
                if !global {
                    break;
                }
            }
            if n > 0 {
                new.push_str(&text[last..]);
                total += n;
                lines_changed += 1;
                if !count_only {
                    let added = new.matches('\n').count();
                    self.set_line(l, &new);
                    end += added;
                    l += added;
                }
                last_line = Some(l);
            }
            l += 1;
        }
        if total == 0 {
            return if flags.contains('e') { Ok(()) } else { Err(format!("E486: Pattern not found: {pat}")) };
        }
        if count_only {
            self.info(format!("{total} match{} on {lines_changed} line{}", if total == 1 { "" } else { "es" }, if lines_changed == 1 { "" } else { "s" }));
            return Ok(());
        }
        if let Some(l) = last_line {
            self.push_jump();
            let c = self.buf().first_non_blank(l);
            self.set_cursor_col(Pos::new(l, c));
        }
        if total > 2 {
            self.info(format!(
                "{total} substitution{} on {lines_changed} line{}",
                if total == 1 { "" } else { "s" },
                if lines_changed == 1 { "" } else { "s" }
            ));
        }
        Ok(())
    }

    fn global(&mut self, l1: usize, l2: usize, args: &str, invert: bool) -> R {
        let chars: Vec<char> = args.chars().collect();
        let d = *chars.first().ok_or("E476: Invalid command")?;
        let mut i = 1;
        let mut pat = String::new();
        while i < chars.len() && chars[i] != d {
            if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == d {
                pat.push(d);
                i += 2;
                continue;
            }
            pat.push(chars[i]);
            i += 1;
        }
        let cmd: String = chars[(i + 1).min(chars.len())..].iter().collect();
        let pat = if pat.is_empty() { self.search.pattern.clone().ok_or("E35: No previous regular expression")? } else { pat };
        self.search.pattern = Some(pat.clone());
        let re = self.build_regex(&pat, true)?;
        let mut targets: Vec<usize> = (l1..=l2).filter(|&l| re.is_match(&self.buf().line(l)) != invert).collect();
        if targets.is_empty() {
            return Err(format!("E486: Pattern not found: {pat}"));
        }
        let cmd = cmd.trim().to_string();
        if cmd.is_empty() || cmd == "p" || cmd == "print" || cmd == "#" {
            let lines = targets
                .iter()
                .map(|&l| if cmd == "#" { format!("{:>3} {}", l + 1, self.buf().line(l)) } else { self.buf().line(l) })
                .collect();
            self.show_lines(lines);
            return Ok(());
        }
        self.begin_change();
        let mut k = 0;
        while k < targets.len() {
            let l = targets[k];
            if l >= self.buf().line_count() {
                break;
            }
            let before = self.buf().line_count() as isize;
            self.win_mut().cursor = Pos::new(l, 0);
            self.run_ex(&cmd)?;
            let delta = self.buf().line_count() as isize - before;
            for t in targets.iter_mut().skip(k + 1) {
                *t = (*t as isize + delta).max(0) as usize;
            }
            k += 1;
        }
        Ok(())
    }
}

#[derive(Debug)]
enum RTok {
    Lit(String),
    Group(usize),
    UpperNext,
    LowerNext,
    UpperAll,
    LowerAll,
    EndCase,
}

fn parse_rep(rep: &str) -> Vec<RTok> {
    let mut out = Vec::new();
    let mut chars = rep.chars().peekable();
    let mut lit = String::new();
    let flush = |lit: &mut String, out: &mut Vec<RTok>| {
        if !lit.is_empty() {
            out.push(RTok::Lit(std::mem::take(lit)));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') | Some('r') => lit.push('\n'),
                Some('t') => lit.push('\t'),
                Some(d @ '0'..='9') => {
                    flush(&mut lit, &mut out);
                    out.push(RTok::Group(d.to_digit(10).unwrap() as usize));
                }
                Some('u') => {
                    flush(&mut lit, &mut out);
                    out.push(RTok::UpperNext);
                }
                Some('l') => {
                    flush(&mut lit, &mut out);
                    out.push(RTok::LowerNext);
                }
                Some('U') => {
                    flush(&mut lit, &mut out);
                    out.push(RTok::UpperAll);
                }
                Some('L') => {
                    flush(&mut lit, &mut out);
                    out.push(RTok::LowerAll);
                }
                Some('e') | Some('E') => {
                    flush(&mut lit, &mut out);
                    out.push(RTok::EndCase);
                }
                Some(o) => lit.push(o),
                None => lit.push('\\'),
            },
            '&' => {
                flush(&mut lit, &mut out);
                out.push(RTok::Group(0));
            }
            o => lit.push(o),
        }
    }
    flush(&mut lit, &mut out);
    out
}

fn expand_rep(toks: &[RTok], caps: &Captures) -> String {
    let mut out = String::new();
    let mut one: Option<bool> = None;
    let mut all: Option<bool> = None;
    let mut push = |s: &str, one: &mut Option<bool>, all: &Option<bool>| {
        for c in s.chars() {
            let c: String = match (one.take(), all) {
                (Some(true), _) => c.to_uppercase().collect(),
                (Some(false), _) => c.to_lowercase().collect(),
                (None, Some(true)) => c.to_uppercase().collect(),
                (None, Some(false)) => c.to_lowercase().collect(),
                (None, None) => c.to_string(),
            };
            out.push_str(&c);
        }
    };
    for t in toks {
        match t {
            RTok::Lit(s) => push(s, &mut one, &all),
            RTok::Group(n) => push(caps.get(*n).map(|m| m.as_str()).unwrap_or(""), &mut one, &all),
            RTok::UpperNext => one = Some(true),
            RTok::LowerNext => one = Some(false),
            RTok::UpperAll => all = Some(true),
            RTok::LowerAll => all = Some(false),
            RTok::EndCase => all = None,
        }
    }
    out
}

pub fn expand_path(s: &str) -> PathBuf {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(h) = dirs::home_dir()
    {
        return h.join(rest);
    }
    if s == "~" {
        return dirs::home_dir().unwrap_or_default();
    }
    PathBuf::from(s.replace("\\ ", " "))
}

fn run_shell(cmd: &str, input: Option<&str>) -> Result<String, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".into());
    let mut child = Command::new(shell)
        .arg("-c")
        .arg(cmd)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("E282: Cannot execute shell: {e}"))?;
    if let Some(inp) = input
        && let Some(mut stdin) = child.stdin.take()
    {
        let data = inp.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(data.as_bytes());
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    if !out.status.success() && s.trim().is_empty() {
        return Err(format!("shell returned {}", out.status.code().unwrap_or(-1)));
    }
    Ok(s)
}
