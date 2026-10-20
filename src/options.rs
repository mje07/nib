//! Editor options (`:set`).

#[derive(Clone, Debug)]
pub struct Options {
    pub number: bool,
    pub relativenumber: bool,
    pub wrap: bool,
    pub linebreak: bool,
    /// Indent wrapped rows to line up with the text (lists, quotes, indents).
    pub breakindent: bool,
    /// Readable line length: wrap at most this many columns, centred in the
    /// window like Obsidian. 0 wraps at the window edge.
    pub wrapwidth: usize,
    pub tabstop: usize,
    pub shiftwidth: usize,
    pub expandtab: bool,
    pub autoindent: bool,
    pub scrolloff: usize,
    pub sidescrolloff: usize,
    pub ignorecase: bool,
    pub smartcase: bool,
    pub hlsearch: bool,
    pub incsearch: bool,
    pub wrapscan: bool,
    pub textwidth: usize,
    /// Markdown live preview.
    pub preview: bool,
    pub cursorline: bool,
    pub showmode: bool,
    pub showcmd: bool,
    pub timeoutlen: u64,
    pub splitbelow: bool,
    pub splitright: bool,
    pub mouse: bool,
    /// Use the system clipboard for the unnamed register.
    pub clipboard: bool,
    pub undolevels: usize,
    /// Scale markdown headings (kitty text-sizing protocol) when supported.
    pub headingscale: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            number: true,
            relativenumber: false,
            wrap: true,
            linebreak: true,
            breakindent: true,
            wrapwidth: 90,
            tabstop: 4,
            shiftwidth: 4,
            expandtab: true,
            autoindent: true,
            scrolloff: 3,
            sidescrolloff: 5,
            ignorecase: true,
            smartcase: true,
            hlsearch: true,
            incsearch: true,
            wrapscan: true,
            textwidth: 80,
            preview: true,
            cursorline: false,
            showmode: true,
            showcmd: true,
            timeoutlen: 1000,
            splitbelow: false,
            splitright: false,
            mouse: true,
            clipboard: false,
            undolevels: 1000,
            headingscale: true,
        }
    }
}

pub enum OptValue {
    Bool(bool),
    Num(usize),
}

/// (canonical name, short alias)
pub const OPTION_NAMES: &[(&str, &str)] = &[
    ("number", "nu"),
    ("relativenumber", "rnu"),
    ("wrap", "wrap"),
    ("linebreak", "lbr"),
    ("breakindent", "bri"),
    ("wrapwidth", "wrw"),
    ("tabstop", "ts"),
    ("shiftwidth", "sw"),
    ("expandtab", "et"),
    ("autoindent", "ai"),
    ("scrolloff", "so"),
    ("sidescrolloff", "siso"),
    ("ignorecase", "ic"),
    ("smartcase", "scs"),
    ("hlsearch", "hls"),
    ("incsearch", "is"),
    ("wrapscan", "ws"),
    ("textwidth", "tw"),
    ("preview", "pv"),
    ("cursorline", "cul"),
    ("showmode", "smd"),
    ("showcmd", "sc"),
    ("timeoutlen", "tm"),
    ("splitbelow", "sb"),
    ("splitright", "spr"),
    ("mouse", "mouse"),
    ("clipboard", "cb"),
    ("undolevels", "ul"),
    ("headingscale", "hsc"),
];

pub fn canonical(name: &str) -> Option<&'static str> {
    OPTION_NAMES.iter().find(|(l, s)| *l == name || *s == name).map(|(l, _)| *l)
}

impl Options {
    pub fn get(&self, name: &str) -> Option<OptValue> {
        use OptValue::*;
        Some(match canonical(name)? {
            "number" => Bool(self.number),
            "relativenumber" => Bool(self.relativenumber),
            "wrap" => Bool(self.wrap),
            "linebreak" => Bool(self.linebreak),
            "breakindent" => Bool(self.breakindent),
            "wrapwidth" => Num(self.wrapwidth),
            "tabstop" => Num(self.tabstop),
            "shiftwidth" => Num(self.shiftwidth),
            "expandtab" => Bool(self.expandtab),
            "autoindent" => Bool(self.autoindent),
            "scrolloff" => Num(self.scrolloff),
            "sidescrolloff" => Num(self.sidescrolloff),
            "ignorecase" => Bool(self.ignorecase),
            "smartcase" => Bool(self.smartcase),
            "hlsearch" => Bool(self.hlsearch),
            "incsearch" => Bool(self.incsearch),
            "wrapscan" => Bool(self.wrapscan),
            "textwidth" => Num(self.textwidth),
            "preview" => Bool(self.preview),
            "cursorline" => Bool(self.cursorline),
            "showmode" => Bool(self.showmode),
            "showcmd" => Bool(self.showcmd),
            "timeoutlen" => Num(self.timeoutlen as usize),
            "splitbelow" => Bool(self.splitbelow),
            "splitright" => Bool(self.splitright),
            "mouse" => Bool(self.mouse),
            "clipboard" => Bool(self.clipboard),
            "undolevels" => Num(self.undolevels),
            "headingscale" => Bool(self.headingscale),
            _ => return None,
        })
    }

    pub fn set_bool(&mut self, name: &str, v: bool) -> Result<(), String> {
        let n = canonical(name).ok_or_else(|| format!("E518: Unknown option: {name}"))?;
        match n {
            "number" => self.number = v,
            "relativenumber" => self.relativenumber = v,
            "wrap" => self.wrap = v,
            "linebreak" => self.linebreak = v,
            "breakindent" => self.breakindent = v,
            "expandtab" => self.expandtab = v,
            "autoindent" => self.autoindent = v,
            "ignorecase" => self.ignorecase = v,
            "smartcase" => self.smartcase = v,
            "hlsearch" => self.hlsearch = v,
            "incsearch" => self.incsearch = v,
            "wrapscan" => self.wrapscan = v,
            "preview" => self.preview = v,
            "cursorline" => self.cursorline = v,
            "showmode" => self.showmode = v,
            "showcmd" => self.showcmd = v,
            "splitbelow" => self.splitbelow = v,
            "splitright" => self.splitright = v,
            "mouse" => self.mouse = v,
            "clipboard" => self.clipboard = v,
            "headingscale" => self.headingscale = v,
            _ => return Err(format!("E474: Invalid argument: {name}")),
        }
        Ok(())
    }

    pub fn set_num(&mut self, name: &str, v: usize) -> Result<(), String> {
        let n = canonical(name).ok_or_else(|| format!("E518: Unknown option: {name}"))?;
        match n {
            "tabstop" => self.tabstop = v.max(1),
            "shiftwidth" => self.shiftwidth = v.max(1),
            "scrolloff" => self.scrolloff = v,
            "sidescrolloff" => self.sidescrolloff = v,
            "textwidth" => self.textwidth = v,
            "wrapwidth" => self.wrapwidth = v,
            "timeoutlen" => self.timeoutlen = v as u64,
            "undolevels" => self.undolevels = v,
            _ => return Err(format!("E474: Invalid argument: {name}={v}")),
        }
        Ok(())
    }

    pub fn is_bool(&self, name: &str) -> bool {
        matches!(self.get(name), Some(OptValue::Bool(_)))
    }
}
