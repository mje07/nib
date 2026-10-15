//! Registers, including the system clipboard (`+`/`*`).

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegKind {
    Char,
    Line,
    Block,
}

#[derive(Clone, Debug)]
pub struct RegVal {
    /// For `Line` registers the text has no trailing newline; lines are `\n`-joined.
    pub text: String,
    pub kind: RegKind,
}

impl RegVal {
    pub fn new(text: impl Into<String>, kind: RegKind) -> Self {
        RegVal { text: text.into(), kind }
    }
}

#[derive(Default)]
pub struct Registers {
    map: HashMap<char, RegVal>,
    pub clipboard: Clipboard,
    /// Mirror unnamed register to the clipboard (`:set clipboard`).
    pub use_clipboard: bool,
}

pub fn valid_register(c: char) -> bool {
    c.is_ascii_alphanumeric() || "\"-_+*/:.%#".contains(c)
}

impl Registers {
    pub fn get(&self, r: char) -> Option<RegVal> {
        let r = r.to_ascii_lowercase();
        match r {
            '_' => None,
            '+' | '*' => self.clipboard.get().or_else(|| self.map.get(&'+').cloned()),
            '"' if self.use_clipboard => self.clipboard.get().or_else(|| self.map.get(&'"').cloned()),
            _ => self.map.get(&r).cloned(),
        }
    }

    /// Set a read-only/special register directly (`.`, `:`, `/`, `%`).
    pub fn set_special(&mut self, r: char, text: &str) {
        self.map.insert(r, RegVal::new(text, RegKind::Char));
    }

    fn write_named(&mut self, r: char, val: &RegVal) -> bool {
        match r {
            '_' => return true,
            'A'..='Z' => {
                let lr = r.to_ascii_lowercase();
                let merged = match self.map.get(&lr) {
                    Some(old) => {
                        let kind = if old.kind == RegKind::Line || val.kind == RegKind::Line {
                            RegKind::Line
                        } else {
                            old.kind
                        };
                        let sep = if old.kind == RegKind::Line || val.kind == RegKind::Line { "\n" } else { "" };
                        RegVal::new(format!("{}{}{}", old.text, sep, val.text), kind)
                    }
                    None => val.clone(),
                };
                self.map.insert(lr, merged.clone());
                self.map.insert('"', merged);
                return false;
            }
            '+' | '*' => {
                self.clipboard.set(&val.text);
                self.map.insert('+', val.clone());
            }
            _ => {
                self.map.insert(r, val.clone());
            }
        }
        self.map.insert('"', val.clone());
        false
    }

    pub fn yank(&mut self, reg: Option<char>, val: RegVal) {
        match reg {
            Some(r) if r != '"' => {
                self.write_named(r, &val);
            }
            _ => {
                self.map.insert('0', val.clone());
                self.map.insert('"', val.clone());
                if self.use_clipboard {
                    self.clipboard.set(&val.text);
                }
            }
        }
    }

    pub fn delete(&mut self, reg: Option<char>, val: RegVal) {
        if reg == Some('_') {
            return;
        }
        if let Some(r) = reg
            && r != '"'
        {
            self.write_named(r, &val);
            return;
        }
        let small = val.kind == RegKind::Char && !val.text.contains('\n');
        if small {
            self.map.insert('-', val.clone());
        } else {
            for i in (1..9).rev() {
                let from = char::from_digit(i, 10).unwrap();
                let to = char::from_digit(i + 1, 10).unwrap();
                if let Some(v) = self.map.get(&from).cloned() {
                    self.map.insert(to, v);
                }
            }
            self.map.insert('1', val.clone());
        }
        self.map.insert('"', val.clone());
        if self.use_clipboard {
            self.clipboard.set(&val.text);
        }
    }

    pub fn list(&self) -> Vec<(char, RegVal)> {
        let order = "\"0123456789abcdefghijklmnopqrstuvwxyz-.:/+";
        order.chars().filter_map(|c| self.map.get(&c).map(|v| (c, v.clone()))).collect()
    }
}

/// System clipboard via external tools, with an OSC 52 fallback for copying.
#[derive(Default)]
pub struct Clipboard {
    probed: std::cell::Cell<bool>,
    tool: std::cell::RefCell<Option<(Vec<&'static str>, Vec<&'static str>)>>,
    /// Text the main loop should send to the terminal via OSC 52.
    pub osc52: std::cell::RefCell<Option<String>>,
    pub disabled: bool,
}

fn have(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

impl Clipboard {
    fn probe(&self) {
        if self.probed.get() {
            return;
        }
        self.probed.set(true);
        let tool = if std::env::var_os("WAYLAND_DISPLAY").is_some() && have("wl-copy") && have("wl-paste") {
            Some((vec!["wl-copy"], vec!["wl-paste", "--no-newline"]))
        } else if std::env::var_os("DISPLAY").is_some() && have("xclip") {
            Some((vec!["xclip", "-selection", "clipboard"], vec!["xclip", "-selection", "clipboard", "-o"]))
        } else if std::env::var_os("DISPLAY").is_some() && have("xsel") {
            Some((vec!["xsel", "-ib"], vec!["xsel", "-ob"]))
        } else if have("pbcopy") {
            Some((vec!["pbcopy"], vec!["pbpaste"]))
        } else {
            None
        };
        *self.tool.borrow_mut() = tool;
    }

    pub fn set(&self, text: &str) {
        if self.disabled {
            return;
        }
        self.probe();
        if let Some((copy, _)) = self.tool.borrow().as_ref()
            && let Ok(mut child) = Command::new(copy[0])
                .args(&copy[1..])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return;
        }
        *self.osc52.borrow_mut() = Some(text.to_string());
    }

    pub fn get(&self) -> Option<RegVal> {
        if self.disabled {
            return None;
        }
        self.probe();
        let tool = self.tool.borrow();
        let (_, paste) = tool.as_ref()?;
        let out = Command::new(paste[0]).args(&paste[1..]).stderr(Stdio::null()).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let mut text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
        let kind = if text.ends_with('\n') {
            text.pop();
            RegKind::Line
        } else {
            RegKind::Char
        };
        Some(RegVal::new(text, kind))
    }
}
