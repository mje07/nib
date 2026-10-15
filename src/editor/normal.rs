//! Normal/visual-mode command grammar: parsing key sequences into commands
//! and executing them.

use super::register::valid_register;
use super::view::ScrollPos;
use super::{Dot, Editor, Mode, VKind};
use crate::key::{Key, KeyCode};
use crate::types::Pos;

#[derive(Clone, Debug, PartialEq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    GDown,
    GUp,
    LineStart,
    FirstNonBlank,
    LineEnd,
    LastNonBlank,
    Column,
    WordFwd(bool),
    WordBack(bool),
    WordEnd(bool),
    WordEndBack(bool),
    Find { ch: char, fwd: bool, till: bool },
    RepeatFind(bool),
    GotoLine,
    GotoFirst,
    Percent,
    MatchPair,
    ParaFwd,
    ParaBack,
    SentFwd,
    SentBack,
    SectionFwd,
    SectionBack,
    ScreenTop,
    ScreenMid,
    ScreenBottom,
    SearchNext(bool),
    Star { fwd: bool },
    /// `None` pattern: prompt for it (operator + `/`).
    Search(Option<String>, bool),
    Mark { ch: char, line: bool },
    NextLineStart,
    PrevLineStart,
    CurLineStart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Delete,
    Change,
    Yank,
    Indent,
    Dedent,
    Lower,
    Upper,
    Toggle,
    Format,
    FormatKeep,
    Rot13,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextObj {
    Word(bool),
    Sentence,
    Paragraph,
    Quote(char),
    Block(char, char),
    Tag,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisualDot {
    pub kind: VKind,
    pub lines: usize,
    pub cols: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Motion(Motion),
    Obj(TextObj, bool),
    Lines,
    Visual(VisualDot),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Move(Motion),
    Op(Op, Target),
    VisualOp(Op),
    VisualOpLines(Op),
    VisualObj(TextObj, bool),
    VisualSwap,
    VisualSwapH,
    VisualBlockInsert(bool),
    VisualReplace(char),
    VisualJoin(bool),
    VisualPut(bool),
    VisualIncr(i64, bool),
    Insert(char),
    ReplaceChar(char),
    ReplaceMode,
    Put { before: bool, gp: bool },
    Join(bool),
    TildeChar,
    Undo,
    Redo,
    RepeatDot,
    Record(char),
    Play(char),
    SetMark(char),
    Incr(i64),
    ScrollHalf(bool),
    ScrollPage(bool),
    ScrollLine(bool),
    ScrollCursor(char),
    Fold(char),
    VisualStart(VKind),
    Reselect,
    Cmdline(char),
    RepeatSub,
    TabNext,
    TabPrev,
    Window(Key),
    WriteQuit,
    QuitForce,
    FileInfo,
    Redraw,
    AltBuf,
    JumpOlder,
    JumpNewer,
    OpenUrl,
    GotoFile,
    Suspend,
    Nop,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cmd {
    pub reg: Option<char>,
    pub count: Option<usize>,
    pub act: Act,
}

impl Cmd {
    /// Whether the command can modify the buffer.
    pub fn is_change(&self) -> bool {
        match &self.act {
            Act::Op(op, _) | Act::VisualOp(op) | Act::VisualOpLines(op) => *op != Op::Yank,
            Act::VisualBlockInsert(_)
            | Act::VisualReplace(_)
            | Act::VisualJoin(_)
            | Act::VisualPut(_)
            | Act::VisualIncr(..)
            | Act::Insert(_)
            | Act::ReplaceChar(_)
            | Act::ReplaceMode
            | Act::Put { .. }
            | Act::Join(_)
            | Act::TildeChar
            | Act::Incr(_)
            | Act::RepeatDot => true,
            _ => false,
        }
    }

    fn dot_repeatable(&self) -> bool {
        self.is_change() && !matches!(self.act, Act::RepeatDot | Act::VisualBlockInsert(_) | Act::VisualIncr(..))
    }
}

pub enum Parse {
    Incomplete,
    Invalid,
    Done(Cmd),
}

pub enum PendState {
    Start,
    Operator,
    Other,
}

enum F {
    Incomplete,
    Invalid,
}

struct P<'a> {
    k: &'a [Key],
    i: usize,
}

impl P<'_> {
    fn next(&mut self) -> Result<Key, F> {
        let k = *self.k.get(self.i).ok_or(F::Incomplete)?;
        self.i += 1;
        Ok(k)
    }
    fn peek(&self) -> Option<Key> {
        self.k.get(self.i).copied()
    }
    fn char_arg(&mut self) -> Result<char, F> {
        let k = self.next()?;
        match k.code {
            KeyCode::Esc => Err(F::Invalid),
            KeyCode::Enter if !k.ctrl => Ok('\r'),
            KeyCode::Tab => Ok('\t'),
            KeyCode::Char(c) if !k.ctrl && !k.alt => Ok(c),
            _ => Err(F::Invalid),
        }
    }
    fn count(&mut self) -> Option<usize> {
        let mut n: Option<usize> = None;
        while let Some(k) = self.peek() {
            match k.char() {
                Some(c @ '0'..='9') if c != '0' || n.is_some() => {
                    n = Some(n.unwrap_or(0).saturating_mul(10).saturating_add(c as usize - '0' as usize).min(99_999_999));
                    self.i += 1;
                }
                _ => break,
            }
        }
        n
    }
    fn done(&self) -> bool {
        self.i >= self.k.len()
    }
}

fn mul(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(1).saturating_mul(b.unwrap_or(1))),
    }
}

pub fn parse(keys: &[Key], visual: bool) -> Parse {
    let mut p = P { k: keys, i: 0 };
    match parse_inner(&mut p, visual) {
        Ok(c) if p.done() => Parse::Done(c),
        Ok(_) => Parse::Invalid,
        Err(F::Incomplete) => Parse::Incomplete,
        Err(F::Invalid) => Parse::Invalid,
    }
}

pub fn pending_state(keys: &[Key]) -> PendState {
    let mut p = P { k: keys, i: 0 };
    if p.peek().and_then(|k| k.char()) == Some('"') {
        p.i += 2;
    }
    p.count();
    if p.done() {
        return PendState::Start;
    }
    let k = p.peek().unwrap();
    let is_op = match k.char() {
        Some('d' | 'c' | 'y' | '<' | '>') => {
            p.i += 1;
            true
        }
        Some('g') => {
            let k2 = p.k.get(p.i + 1).and_then(|k| k.char());
            if matches!(k2, Some('~' | 'u' | 'U' | 'q' | 'w' | '?')) {
                p.i += 2;
                true
            } else {
                false
            }
        }
        _ => false,
    };
    if !is_op {
        return PendState::Other;
    }
    p.count();
    if p.done() { PendState::Operator } else { PendState::Other }
}

fn op_key(k: Key, p: &mut P) -> Result<Option<(Op, char)>, F> {
    Ok(match k.char() {
        Some('d') => Some((Op::Delete, 'd')),
        Some('c') => Some((Op::Change, 'c')),
        Some('y') => Some((Op::Yank, 'y')),
        Some('>') => Some((Op::Indent, '>')),
        Some('<') => Some((Op::Dedent, '<')),
        Some('g') => {
            let Some(k2) = p.peek() else { return Err(F::Incomplete) };
            let op = match k2.char() {
                Some('~') => Op::Toggle,
                Some('u') => Op::Lower,
                Some('U') => Op::Upper,
                Some('q') => Op::Format,
                Some('w') => Op::FormatKeep,
                Some('?') => Op::Rot13,
                _ => return Ok(None),
            };
            p.i += 1;
            Some((op, k2.char().unwrap()))
        }
        _ => None,
    })
}

fn parse_obj(p: &mut P) -> Result<TextObj, F> {
    let c = p.char_arg()?;
    Ok(match c {
        'w' => TextObj::Word(false),
        'W' => TextObj::Word(true),
        's' => TextObj::Sentence,
        'p' => TextObj::Paragraph,
        '"' | '\'' | '`' => TextObj::Quote(c),
        '(' | ')' | 'b' => TextObj::Block('(', ')'),
        '[' | ']' => TextObj::Block('[', ']'),
        '{' | '}' | 'B' => TextObj::Block('{', '}'),
        '<' | '>' => TextObj::Block('<', '>'),
        't' => TextObj::Tag,
        _ => return Err(F::Invalid),
    })
}

fn parse_motion(k: Key, p: &mut P) -> Result<Option<Motion>, F> {
    use Motion::*;
    if k.ctrl {
        return Ok(match k.code {
            KeyCode::Char('h') => Some(Left),
            KeyCode::Char('j') | KeyCode::Char('n') => Some(Down),
            KeyCode::Char('p') => Some(Up),
            KeyCode::Char('m') => Some(NextLineStart),
            _ => None,
        });
    }
    Ok(Some(match k.code {
        KeyCode::Left | KeyCode::Backspace => Left,
        KeyCode::Right => Right,
        KeyCode::Up => Up,
        KeyCode::Down => Down,
        KeyCode::Home => LineStart,
        KeyCode::End => LineEnd,
        KeyCode::Enter => NextLineStart,
        KeyCode::Char(c) => match c {
            'h' => Left,
            'l' | ' ' => Right,
            'j' => Down,
            'k' => Up,
            '0' => LineStart,
            '^' => FirstNonBlank,
            '$' => LineEnd,
            '|' => Column,
            'w' => WordFwd(false),
            'W' => WordFwd(true),
            'b' => WordBack(false),
            'B' => WordBack(true),
            'e' => WordEnd(false),
            'E' => WordEnd(true),
            'f' | 'F' | 't' | 'T' => {
                let ch = p.char_arg()?;
                Find { ch, fwd: c == 'f' || c == 't', till: c == 't' || c == 'T' }
            }
            ';' => RepeatFind(false),
            ',' => RepeatFind(true),
            'G' => GotoLine,
            '%' => MatchPair,
            '}' => ParaFwd,
            '{' => ParaBack,
            ')' => SentFwd,
            '(' => SentBack,
            'H' => ScreenTop,
            'M' => ScreenMid,
            'L' => ScreenBottom,
            'n' => SearchNext(false),
            'N' => SearchNext(true),
            '*' => Star { fwd: true },
            '#' => Star { fwd: false },
            '/' => Search(None, true),
            '?' => Search(None, false),
            '\'' | '`' => {
                let ch = p.char_arg()?;
                Mark { ch, line: c == '\'' }
            }
            '+' => NextLineStart,
            '-' => PrevLineStart,
            '_' => CurLineStart,
            ']' | '[' => {
                let c2 = p.char_arg()?;
                match (c, c2) {
                    (']', ']') | (']', '[') => SectionFwd,
                    ('[', '[') | ('[', ']') => SectionBack,
                    _ => return Err(F::Invalid),
                }
            }
            'g' => {
                let Some(k2) = p.peek() else { return Err(F::Incomplete) };
                let m = match k2.code {
                    KeyCode::Char('g') => GotoFirst,
                    KeyCode::Char('e') => WordEndBack(false),
                    KeyCode::Char('E') => WordEndBack(true),
                    KeyCode::Char('j') | KeyCode::Down => GDown,
                    KeyCode::Char('k') | KeyCode::Up => GUp,
                    KeyCode::Char('_') => LastNonBlank,
                    KeyCode::Char('0') | KeyCode::Home => LineStart,
                    KeyCode::Char('$') | KeyCode::End => LineEnd,
                    _ => return Ok(None),
                };
                p.i += 1;
                m
            }
            _ => return Ok(None),
        },
        _ => return Ok(None),
    }))
}

fn parse_inner(p: &mut P, visual: bool) -> Result<Cmd, F> {
    let mut reg = None;
    if p.peek().and_then(|k| k.char()) == Some('"') {
        p.i += 1;
        let r = p.char_arg()?;
        if !valid_register(r) {
            return Err(F::Invalid);
        }
        reg = Some(r);
    }
    let c1 = p.count();
    let k = p.next()?;
    let cmd = |act| Ok(Cmd { reg, count: c1, act });

    // Operators.
    let save = p.i;
    if let Some((op, ch)) = op_key(k, p)? {
        if visual {
            return cmd(Act::VisualOp(op));
        }
        let c2 = p.count();
        let count = mul(c1, c2);
        let k2 = p.next()?;
        let mk = |t| Ok(Cmd { reg, count, act: Act::Op(op, t) });
        let is_g = k.char() == Some('g');
        if k2.char() == Some(ch) {
            return mk(Target::Lines);
        }
        if is_g && k2.char() == Some('g') {
            let k3 = p.next()?;
            if k3.char() == Some(ch) {
                return mk(Target::Lines);
            }
            p.i -= 1;
        }
        if matches!(k2.char(), Some('i') | Some('a')) {
            let obj = parse_obj(p)?;
            return mk(Target::Obj(obj, k2.char() == Some('i')));
        }
        // Special forms: cw behaves like ce (handled in the operator code).
        return match parse_motion(k2, p)? {
            Some(m) => mk(Target::Motion(m)),
            None => Err(F::Invalid),
        };
    }
    p.i = save;

    if visual && let Some(act) = parse_visual_only(k, p)? {
        return cmd(act);
    }

    // Shorthands for operators.
    let c = k.char();
    let sh = |op, t| Ok(Cmd { reg, count: c1, act: Act::Op(op, t) });
    if !visual {
        match c {
            Some('x') => return sh(Op::Delete, Target::Motion(Motion::Right)),
            Some('X') => return sh(Op::Delete, Target::Motion(Motion::Left)),
            Some('D') => return sh(Op::Delete, Target::Motion(Motion::LineEnd)),
            Some('C') => return sh(Op::Change, Target::Motion(Motion::LineEnd)),
            Some('s') => return sh(Op::Change, Target::Motion(Motion::Right)),
            Some('S') => return sh(Op::Change, Target::Lines),
            Some('Y') => return sh(Op::Yank, Target::Motion(Motion::LineEnd)),
            _ => {}
        }
        if k.code == KeyCode::Delete {
            return sh(Op::Delete, Target::Motion(Motion::Right));
        }
    }

    // Count-prefixed % goes to a percentage of the file.
    if c == Some('%') && c1.is_some() {
        return cmd(Act::Move(Motion::Percent));
    }

    if let Some(m) = parse_motion(k, p)? {
        return cmd(Act::Move(m));
    }

    if k.ctrl {
        let act = match k.code {
            KeyCode::Char('r') => Act::Redo,
            KeyCode::Char('a') => Act::Incr(1),
            KeyCode::Char('x') => Act::Incr(-1),
            KeyCode::Char('d') => Act::ScrollHalf(true),
            KeyCode::Char('u') => Act::ScrollHalf(false),
            KeyCode::Char('f') => Act::ScrollPage(true),
            KeyCode::Char('b') => Act::ScrollPage(false),
            KeyCode::Char('e') => Act::ScrollLine(true),
            KeyCode::Char('y') => Act::ScrollLine(false),
            KeyCode::Char('v') | KeyCode::Char('q') => Act::VisualStart(VKind::Block),
            KeyCode::Char('g') => Act::FileInfo,
            KeyCode::Char('l') => Act::Redraw,
            KeyCode::Char('^') | KeyCode::Char('6') => Act::AltBuf,
            KeyCode::Char('o') => Act::JumpOlder,
            KeyCode::Char('i') => Act::JumpNewer,
            KeyCode::Char('z') => Act::Suspend,
            KeyCode::Char('c') => Act::Nop,
            KeyCode::Char('w') => {
                let k2 = p.next()?;
                Act::Window(k2)
            }
            _ => return Err(F::Invalid),
        };
        return cmd(act);
    }

    let act = match k.code {
        KeyCode::Tab => Act::JumpNewer,
        KeyCode::Insert => Act::Insert('i'),
        KeyCode::PageDown => Act::ScrollPage(true),
        KeyCode::PageUp => Act::ScrollPage(false),
        KeyCode::Char(ch) if !k.alt => match ch {
            'i' | 'a' | 'I' | 'A' | 'o' | 'O' => Act::Insert(ch),
            'r' => Act::ReplaceChar(p.char_arg()?),
            'R' => Act::ReplaceMode,
            'p' => Act::Put { before: false, gp: false },
            'P' => Act::Put { before: true, gp: false },
            'J' => Act::Join(true),
            '~' => Act::TildeChar,
            'u' => Act::Undo,
            '.' => Act::RepeatDot,
            'q' => {
                // Stopping is handled before parsing (see `normal_key`).
                let r = p.char_arg()?;
                if !(r.is_ascii_alphanumeric() || r == '"') {
                    return Err(F::Invalid);
                }
                Act::Record(r)
            }
            '@' => {
                let r = p.char_arg()?;
                Act::Play(r)
            }
            'm' => Act::SetMark(p.char_arg()?),
            'v' => Act::VisualStart(VKind::Char),
            'V' => Act::VisualStart(VKind::Line),
            ':' => Act::Cmdline(':'),
            '&' => Act::RepeatSub,
            'z' => {
                let k2 = p.next()?;
                match k2.code {
                    KeyCode::Enter => Act::ScrollCursor('\r'),
                    KeyCode::Char(c @ ('z' | 't' | 'b' | '.' | '-')) => Act::ScrollCursor(c),
                    KeyCode::Char(c @ ('a' | 'c' | 'o' | 'R' | 'M' | 'A' | 'O' | 'C' | 'v')) => Act::Fold(c),
                    _ => return Err(F::Invalid),
                }
            }
            'Z' => match p.char_arg()? {
                'Z' => Act::WriteQuit,
                'Q' => Act::QuitForce,
                _ => return Err(F::Invalid),
            },
            'g' => {
                let k2 = p.next()?;
                match k2.code {
                    KeyCode::Char('i') => Act::Insert('g'),
                    KeyCode::Char('I') => Act::Insert('G'),
                    KeyCode::Char('p') => Act::Put { before: false, gp: true },
                    KeyCode::Char('P') => Act::Put { before: true, gp: true },
                    KeyCode::Char('J') => Act::Join(false),
                    KeyCode::Char('v') => Act::Reselect,
                    KeyCode::Char('t') => Act::TabNext,
                    KeyCode::Char('T') => Act::TabPrev,
                    KeyCode::Char('x') => Act::OpenUrl,
                    KeyCode::Char('f') => Act::GotoFile,
                    _ => return Err(F::Invalid),
                }
            }
            _ => return Err(F::Invalid),
        },
        _ => return Err(F::Invalid),
    };
    cmd(act)
}

fn parse_visual_only(k: Key, p: &mut P) -> Result<Option<Act>, F> {
    if k.ctrl {
        return Ok(match k.code {
            KeyCode::Char('a') => Some(Act::VisualIncr(1, false)),
            KeyCode::Char('x') => Some(Act::VisualIncr(-1, false)),
            _ => None,
        });
    }
    let Some(c) = k.char() else {
        if k.code == KeyCode::Delete {
            return Ok(Some(Act::VisualOp(Op::Delete)));
        }
        return Ok(None);
    };
    Ok(Some(match c {
        'x' => Act::VisualOp(Op::Delete),
        's' => Act::VisualOp(Op::Change),
        'X' | 'D' => Act::VisualOpLines(Op::Delete),
        'Y' => Act::VisualOpLines(Op::Yank),
        'C' | 'S' | 'R' => Act::VisualOpLines(Op::Change),
        'u' => Act::VisualOp(Op::Lower),
        'U' => Act::VisualOp(Op::Upper),
        '~' => Act::VisualOp(Op::Toggle),
        'J' => Act::VisualJoin(true),
        'o' => Act::VisualSwap,
        'O' => Act::VisualSwapH,
        'I' => Act::VisualBlockInsert(false),
        'A' => Act::VisualBlockInsert(true),
        'r' => Act::VisualReplace(p.char_arg()?),
        'p' => Act::VisualPut(true),
        'P' => Act::VisualPut(false),
        'i' | 'a' => {
            let obj = parse_obj(p)?;
            Act::VisualObj(obj, c == 'i')
        }
        'g' => {
            let Some(k2) = p.peek() else { return Err(F::Incomplete) };
            let act = match k2.code {
                KeyCode::Char('J') => Act::VisualJoin(false),
                KeyCode::Char('a') if k2.ctrl => Act::VisualIncr(1, true),
                KeyCode::Char('x') if k2.ctrl => Act::VisualIncr(-1, true),
                _ => return Ok(None),
            };
            p.i += 1;
            act
        }
        _ => return Ok(None),
    }))
}

// --- Execution -----------------------------------------------------------------

impl Editor {
    pub fn normal_key(&mut self, key: Key) {
        let visual = self.mode.is_visual();
        if (key == Key::ESC || key.is_ctrl('c')) && !self.pending.is_empty() {
            self.pending.clear();
            return;
        }
        if key == Key::ESC || key.is_ctrl('c') {
            if visual {
                self.exit_visual();
            } else if key.is_ctrl('c') {
                self.info("Type  :qa!  and press <Enter> to abandon all changes and exit");
            }
            self.after_normal_cmd();
            return;
        }
        // `q` while recording stops it (cannot be parsed as `q{reg}`).
        if self.pending.is_empty() && key.char() == Some('q') && self.recording.is_some() {
            self.stop_recording();
            return;
        }
        self.pending.push(key);
        match parse(&self.pending, visual) {
            Parse::Incomplete => {}
            Parse::Invalid => self.pending.clear(),
            Parse::Done(cmd) => {
                self.pending.clear();
                self.execute(cmd);
                self.after_normal_cmd();
            }
        }
    }

    fn after_normal_cmd(&mut self) {
        // Ctrl-O from insert mode: return to insert after one command.
        if self.insert.ctrl_o && self.mode == Mode::Normal && self.pending.is_empty() {
            self.insert.ctrl_o = false;
            self.mode = Mode::Insert;
        }
    }

    pub fn execute(&mut self, cmd: Cmd) {
        let count = cmd.count;
        let n = count.unwrap_or(1);
        let change = cmd.is_change();
        if change && !self.buf_mut().history.has_open_changes() {
            self.begin_change();
        }
        self.last_visual_dot = None;
        match cmd.act.clone() {
            Act::Move(m) => match m {
                Motion::GDown | Motion::GUp if self.mode == Mode::Normal || self.mode.is_visual() => {
                    self.display_move(m == Motion::GDown, n)
                }
                _ => self.do_motion(&m, count),
            },
            Act::Op(op, target) => self.op_cmd(op, &target, count, cmd.reg, &cmd),
            Act::VisualOp(op) => self.visual_op(op, false, cmd.reg, n),
            Act::VisualOpLines(op) => self.visual_op(op, true, cmd.reg, n),
            Act::VisualObj(obj, inner) => self.visual_obj(&obj, inner, n),
            Act::VisualSwap => self.visual_swap(false),
            Act::VisualSwapH => self.visual_swap(true),
            Act::VisualBlockInsert(append) => self.visual_block_insert(append),
            Act::VisualReplace(c) => self.visual_replace(c),
            Act::VisualJoin(spaces) => self.visual_join(spaces),
            Act::VisualPut(swap) => self.visual_put(cmd.reg, swap),
            Act::VisualIncr(d, progressive) => self.visual_incr(d * n as i64, progressive),
            Act::Insert(kind) => self.start_insert(kind, n),
            Act::ReplaceChar(c) => self.replace_chars(c, n),
            Act::ReplaceMode => self.start_replace(n),
            Act::Put { before, gp } => self.put(cmd.reg, before, gp, n),
            Act::Join(spaces) => {
                let l = self.cursor().line;
                self.join_lines(l, n.max(2) - 1, spaces);
            }
            Act::TildeChar => self.tilde(n),
            Act::Undo => self.undo(n),
            Act::Redo => self.redo(n),
            Act::RepeatDot => self.repeat_dot(count),
            Act::Record(r) => {
                let r = if r == '"' { '"' } else { r };
                self.recording = Some((r, Vec::new()));
            }
            Act::Play(r) => self.play_macro(r, n),
            Act::SetMark(c) => {
                if !self.set_mark(c) {
                    self.error("E191: Argument must be a letter or forward/backward quote");
                }
            }
            Act::Incr(d) => self.increment(d * n as i64),
            Act::ScrollHalf(down) => {
                let half = (self.text_rect(self.win_id()).h as usize / 2).max(1);
                let amount = count.unwrap_or(half) as isize;
                self.scroll_and_move(if down { amount } else { -amount });
            }
            Act::ScrollPage(down) => {
                let page = self.visible_line_list().len().saturating_sub(2).max(1) as isize * n as isize;
                self.scroll_and_move(if down { page } else { -page });
            }
            Act::ScrollLine(down) => self.scroll_lines(if down { n as isize } else { -(n as isize) }),
            Act::ScrollCursor(c) => {
                if let Some(cnt) = count {
                    let p = Pos::new(cnt.saturating_sub(1), self.cursor().col);
                    self.set_cursor(p);
                }
                let at = match c {
                    't' | '\r' => ScrollPos::Top,
                    'b' | '-' => ScrollPos::Bottom,
                    _ => ScrollPos::Center,
                };
                self.scroll_cursor_to(at);
                if matches!(c, '\r' | '.' | '-') {
                    let l = self.cursor().line;
                    let c = self.buf().first_non_blank(l);
                    self.set_cursor_col(Pos::new(l, c));
                }
            }
            Act::Fold(c) => self.fold_cmd(c),
            Act::VisualStart(kind) => self.visual_start(kind, count),
            Act::Reselect => self.reselect(),
            Act::Cmdline(c) => {
                let initial = if self.mode.is_visual() {
                    "'<,'>".to_string()
                } else if let Some(n) = count {
                    if n == 1 { ".".to_string() } else { format!(".,.+{}", n - 1) }
                } else {
                    String::new()
                };
                self.start_cmdline(c, &initial);
            }
            Act::RepeatSub => {
                let _ = self.run_ex("s");
            }
            Act::TabNext => {
                if let Some(c) = count {
                    self.goto_tab(c.saturating_sub(1));
                } else {
                    self.goto_tab((self.tab + 1) % self.tabs.len());
                }
            }
            Act::TabPrev => {
                let len = self.tabs.len();
                self.goto_tab((self.tab + len - n % len) % len);
            }
            Act::Window(k) => self.window_cmd(k, count),
            Act::WriteQuit => {
                let _ = self.run_ex("x");
            }
            Act::QuitForce => {
                let _ = self.run_ex("q!");
            }
            Act::FileInfo => {
                let s = self.file_info();
                self.info(s);
            }
            Act::Redraw => self.messages.clear(),
            Act::AltBuf => {
                let target = match count {
                    Some(c) if self.buffers.contains_key(&c) => Some(c),
                    _ => self.win().alt_buf.filter(|b| self.buffers.contains_key(b)),
                };
                match target {
                    Some(b) => self.switch_buffer(b),
                    None => self.error("E23: No alternate file"),
                }
            }
            Act::JumpOlder => self.jump_older(n),
            Act::JumpNewer => self.jump_newer(n),
            Act::OpenUrl => self.open_url_under_cursor(),
            Act::GotoFile => self.goto_file_under_cursor(),
            Act::Suspend => self.suspend = true,
            Act::Nop => {}
        }
        if !self.replaying_dot && cmd.dot_repeatable() {
            let dot_cmd = match (&cmd.act, self.last_visual_dot.clone()) {
                (Act::VisualOp(op) | Act::VisualOpLines(op), Some(vd)) => {
                    Some(Cmd { reg: cmd.reg, count: None, act: Act::Op(*op, Target::Visual(vd)) })
                }
                (Act::VisualOp(_) | Act::VisualOpLines(_), None) => None,
                (Act::VisualReplace(_) | Act::VisualJoin(_) | Act::VisualPut(_), _) => None,
                _ => Some(cmd.clone()),
            };
            if let Some(dc) = dot_cmd {
                if self.mode.is_insert() {
                    self.dot_pending = Some(dc);
                } else {
                    self.dot = Some(Dot { cmd: dc, insert_keys: None });
                }
            }
        }
        if !self.mode.is_insert() && self.mode != Mode::Cmdline {
            self.commit();
        }
    }

    fn scroll_and_move(&mut self, lines: isize) {
        let buf_lines = self.buf().line_count();
        let cur = self.cursor().line as isize;
        let target = (cur + lines).clamp(0, buf_lines as isize - 1) as usize;
        if target == cur as usize && ((lines > 0 && target + 1 >= buf_lines) || (lines < 0 && target == 0)) {
            return;
        }
        self.scroll_lines(lines);
        let col = self.col_for_want(target);
        self.win_mut().cursor = Pos::new(target, col);
        let fnb = self.buf().first_non_blank(target);
        self.set_cursor_col(Pos::new(target, fnb));
    }

    fn repeat_dot(&mut self, count: Option<usize>) {
        let Some(dot) = self.dot.clone() else { return };
        let mut c = dot.cmd.clone();
        if count.is_some() {
            c.count = count;
        }
        self.replaying_dot = true;
        self.execute(c);
        if let Some(keys) = &dot.insert_keys
            && self.mode.is_insert()
        {
            self.insert.replaying = true;
            for k in keys {
                self.insert_key(*k);
            }
            self.insert_key(Key::ESC);
            self.insert.replaying = false;
        }
        self.replaying_dot = false;
        if count.is_some()
            && let Some(d) = &mut self.dot
        {
            d.cmd.count = count;
        }
    }

    fn play_macro(&mut self, r: char, n: usize) {
        let r = match r {
            '@' => match self.last_macro {
                Some(r) => r,
                None => {
                    self.error("E748: No previously used register");
                    return;
                }
            },
            ':' => {
                if let Some(cmd) = self.last_ex.clone() {
                    for _ in 0..n {
                        if let Err(e) = self.run_ex(&cmd) {
                            self.error(e);
                            break;
                        }
                    }
                }
                return;
            }
            r => r,
        };
        self.last_macro = Some(r);
        let Some(val) = self.registers.get(r) else { return };
        let mut keys = crate::key::register_to_keys(&val.text);
        if val.kind == super::register::RegKind::Line {
            keys.push(Key::ENTER);
        }
        let mut all = Vec::new();
        for _ in 0..n {
            all.extend(keys.iter().copied());
        }
        for k in all.into_iter().rev() {
            self.typeahead_push_front(k);
        }
    }

    pub fn typeahead_push_front(&mut self, k: Key) {
        self.typeahead.push_front((k, true));
    }

    fn undo(&mut self, n: usize) {
        self.commit();
        let mut last = None;
        for _ in 0..n {
            match self.buf_mut().undo() {
                Some(p) => last = Some(p),
                None => break,
            }
        }
        match last {
            Some(p) => {
                self.set_cursor_col(p);
                let left = self.buf().history.undo_len();
                self.info(format!("{} change{}; before #{}", n, if n == 1 { "" } else { "s" }, left + 1));
            }
            None => self.info("Already at oldest change"),
        }
    }

    fn redo(&mut self, n: usize) {
        let mut last = None;
        for _ in 0..n {
            match self.buf_mut().redo() {
                Some(p) => last = Some(p),
                None => break,
            }
        }
        match last {
            Some(p) => self.set_cursor_col(p),
            None => self.info("Already at newest change"),
        }
    }

    fn fold_cmd(&mut self, c: char) {
        let line = self.cursor().line;
        let buf = self.buf();
        if !buf.is_markdown() {
            self.error("E490: No fold found");
            return;
        }
        let doc = buf.markdown();
        // The heading that owns the cursor line.
        let heading = (0..=line).rev().find(|&l| doc.heading_level(l).is_some() && self.fold_end(buf, l).is_some_and(|e| e >= line));
        match c {
            'R' => self.buf_mut().folds.clear(),
            'M' => {
                let heads: Vec<usize> = (0..buf.line_count()).filter(|&l| doc.heading_level(l).is_some()).collect();
                let b = self.buf_mut();
                b.folds.extend(heads);
                let l = self.cursor().line;
                let v = self.visible_line(self.buf(), l);
                self.set_cursor(Pos::new(v, 0));
            }
            'v' => self.open_folds_at(line),
            _ => {
                let Some(h) = heading else {
                    self.error("E490: No fold found");
                    return;
                };
                // A closed fold containing the cursor takes precedence.
                let closed = self.closed_fold_at(self.buf(), line).map(|(s, _)| s);
                let target = closed.unwrap_or(h);
                let is_closed = self.buf().folds.contains(&target) && closed.is_some();
                match c {
                    'a' | 'A' => {
                        if is_closed {
                            self.buf_mut().folds.remove(&target);
                        } else {
                            self.buf_mut().folds.insert(h);
                        }
                    }
                    'c' | 'C' => {
                        self.buf_mut().folds.insert(h);
                    }
                    'o' | 'O' => {
                        if c == 'O' {
                            self.open_folds_at(line);
                        } else {
                            self.buf_mut().folds.remove(&target);
                        }
                    }
                    _ => {}
                }
                let v = self.visible_line(self.buf(), self.cursor().line);
                if v != self.cursor().line {
                    let c = self.buf().first_non_blank(v);
                    self.set_cursor(Pos::new(v, c));
                }
            }
        }
    }

    fn open_url_under_cursor(&mut self) {
        let Some(target) = self.link_under_cursor() else {
            self.error("No URL under cursor");
            return;
        };
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        match std::process::Command::new(opener)
            .arg(&target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => self.info(format!("Opening {target}")),
            Err(e) => self.error(format!("{opener}: {e}")),
        }
    }

    fn goto_file_under_cursor(&mut self) {
        let Some(target) = self.link_under_cursor() else {
            self.error("E446: No file name under cursor");
            return;
        };
        let target = target.split('#').next().unwrap_or("").to_string();
        let base = self.buf().path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf()));
        let mut path = std::path::PathBuf::from(&target);
        if path.is_relative()
            && let Some(b) = base
        {
            path = b.join(path);
        }
        if !path.exists() && path.extension().is_none() {
            let md = path.with_extension("md");
            if md.exists() {
                path = md;
            }
        }
        let _ = self.run_ex(&format!("e {}", path.display()));
    }

    /// Link target (markdown link destination, autolink or bare word) at the cursor.
    pub fn link_under_cursor(&self) -> Option<String> {
        let line = self.buf().line(self.cursor().line);
        let col = self.cursor().col;
        let chars: Vec<char> = line.chars().collect();
        // [text](dest) containing the cursor.
        let re = regex::Regex::new(r"\[[^\]]*\]\(([^)\s]+)(?:\s+[^)]*)?\)|<(https?://[^>]+)>|(https?://[^\s)>\]]+)").ok()?;
        for m in re.captures_iter(&line) {
            let whole = m.get(0)?;
            let s = line[..whole.start()].chars().count();
            let e = s + whole.as_str().chars().count();
            if col >= s && col < e {
                let t = m.get(1).or(m.get(2)).or(m.get(3))?;
                return Some(t.as_str().to_string());
            }
        }
        // Fallback: the non-blank word under the cursor.
        if col >= chars.len() || chars[col].is_whitespace() {
            return None;
        }
        let mut a = col;
        while a > 0 && !chars[a - 1].is_whitespace() {
            a -= 1;
        }
        let mut b = col;
        while b < chars.len() && !chars[b].is_whitespace() {
            b += 1;
        }
        let w: String = chars[a..b].iter().collect();
        Some(w.trim_matches(|c: char| "()[]<>\"',".contains(c)).to_string())
    }
}
