//! Window and tab-page management (`:split`, `<C-w>` commands, tabs).

use super::layout::{self, Dir, Node};
use super::{Editor, Tab};
use crate::key::{Key, KeyCode};
use crate::types::{BufId, WinId};

impl Editor {
    /// Split the current window; the new window shows `buf` (or the same buffer).
    pub fn split(&mut self, dir: Dir, buf: Option<BufId>) -> WinId {
        let cur = self.win_id();
        let (b, cursor, top, want, preview, jumps, jump_idx) = {
            let w = self.win();
            (w.buf, w.cursor, w.top, w.want_col, w.preview, w.jumps.clone(), w.jump_idx)
        };
        let new = self.new_window(buf.unwrap_or(b));
        {
            let w = self.windows.get_mut(&new).unwrap();
            if buf.is_none() || buf == Some(b) {
                w.cursor = cursor;
                w.top = top;
                w.want_col = want;
            }
            w.preview = preview;
            w.jumps = jumps;
            w.jump_idx = jump_idx;
            if buf.is_some_and(|x| x != b) {
                w.alt_buf = Some(b);
            }
        }
        let after = match dir {
            Dir::Horizontal => self.options.splitbelow,
            Dir::Vertical => self.options.splitright,
        };
        let tab = self.tab_mut();
        layout::split(&mut tab.root, cur, new, dir, after);
        tab.prev = Some(cur);
        tab.active = new;
        if let Some(bid) = buf {
            let p = self.buffers[&bid].last_cursor;
            self.set_cursor_col(p);
        }
        self.scroll_to_cursor();
        new
    }

    pub fn focus_window(&mut self, id: WinId) {
        if id == self.win_id() || !layout::contains(&self.tab().root, id) {
            return;
        }
        self.commit();
        let cur = self.win_id();
        let tab = self.tab_mut();
        tab.prev = Some(cur);
        tab.active = id;
    }

    /// Close window `id`. Returns false if it is the last window of the last tab.
    pub fn close_window(&mut self, id: WinId) -> bool {
        let leaves = layout::leaves(&self.tab().root);
        if leaves.len() == 1 {
            if self.tabs.len() == 1 {
                return false;
            }
            self.close_tab(self.tab);
            return true;
        }
        let idx = leaves.iter().position(|w| *w == id).unwrap_or(0);
        let buf = self.windows[&id].buf;
        let cursor = self.windows[&id].cursor;
        if let Some(b) = self.buffers.get_mut(&buf) {
            b.last_cursor = cursor;
        }
        let tab = self.tab_mut();
        layout::remove(&mut tab.root, id);
        let remaining = layout::leaves(&tab.root);
        let next = tab
            .prev
            .filter(|p| *p != id && remaining.contains(p))
            .unwrap_or_else(|| remaining[idx.min(remaining.len() - 1).saturating_sub(if idx > 0 { 1 } else { 0 })]);
        tab.active = next;
        tab.prev = None;
        self.windows.remove(&id);
        true
    }

    pub fn only_window(&mut self) {
        let cur = self.win_id();
        for id in layout::leaves(&self.tab().root) {
            if id != cur {
                self.windows.remove(&id);
            }
        }
        let tab = self.tab_mut();
        tab.root = Node::Leaf(cur);
        tab.prev = None;
    }

    pub fn new_tab(&mut self, buf: BufId) {
        self.commit();
        let w = self.new_window(buf);
        let p = self.buffers[&buf].last_cursor;
        self.tabs.insert(self.tab + 1, Tab { root: Node::Leaf(w), active: w, prev: None });
        self.tab += 1;
        self.set_cursor_col(p);
    }

    pub fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 {
            return;
        }
        let tab = self.tabs.remove(idx);
        for w in layout::leaves(&tab.root) {
            self.windows.remove(&w);
        }
        if self.tab >= self.tabs.len() || self.tab > idx {
            self.tab = self.tab.saturating_sub(1);
        }
        self.tab = self.tab.min(self.tabs.len() - 1);
    }

    pub fn goto_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.commit();
            self.tab = idx;
        }
    }

    fn window_in_dir(&self, d: char) -> Option<WinId> {
        let rects = layout::rects(&self.tab().root, self.editor_area());
        let cur = self.win_id();
        let (_, cr) = *rects.iter().find(|(w, _)| *w == cur)?;
        let cursor_y = cr.y + 1;
        let cursor_x = cr.x + 1;
        let mut best: Option<(WinId, i32)> = None;
        for (w, r) in &rects {
            if *w == cur {
                continue;
            }
            let v_overlap = r.y < cr.y + cr.h && r.y + r.h > cr.y;
            let h_overlap = r.x < cr.x + cr.w && r.x + r.w > cr.x;
            let ok = match d {
                'h' => r.x + r.w <= cr.x && v_overlap,
                'l' => r.x >= cr.x + cr.w && v_overlap,
                'k' => r.y + r.h <= cr.y && h_overlap,
                'j' => r.y >= cr.y + cr.h && h_overlap,
                _ => false,
            };
            if !ok {
                continue;
            }
            // Prefer the nearest window, then the one overlapping the cursor.
            let dist = match d {
                'h' => (cr.x as i32) - (r.x + r.w) as i32,
                'l' => r.x as i32 - (cr.x + cr.w) as i32,
                'k' => cr.y as i32 - (r.y + r.h) as i32,
                _ => r.y as i32 - (cr.y + cr.h) as i32,
            } * 1000
                + match d {
                    'h' | 'l' => ((r.y as i32 - cursor_y as i32).abs()).min(999),
                    _ => ((r.x as i32 - cursor_x as i32).abs()).min(999),
                };
            if best.is_none_or(|(_, bd)| dist < bd) {
                best = Some((*w, dist));
            }
        }
        best.map(|(w, _)| w)
    }

    pub fn window_cmd(&mut self, k: Key, count: Option<usize>) {
        let c = match k.code {
            KeyCode::Char(c) => c,
            KeyCode::Left => 'h',
            KeyCode::Right => 'l',
            KeyCode::Up => 'k',
            KeyCode::Down => 'j',
            _ => return,
        };
        let n = count.unwrap_or(1);
        let leaves = layout::leaves(&self.tab().root);
        let cur = self.win_id();
        let pos = leaves.iter().position(|w| *w == cur).unwrap_or(0);
        match c {
            's' | 'S' => {
                self.split(Dir::Horizontal, None);
            }
            'v' => {
                self.split(Dir::Vertical, None);
            }
            'n' => {
                let b = self.new_buffer();
                self.split(Dir::Horizontal, Some(b));
            }
            'w' => {
                let target = match count {
                    Some(i) => leaves.get(i - 1).copied(),
                    None => Some(leaves[(pos + 1) % leaves.len()]),
                };
                if let Some(t) = target {
                    self.focus_window(t);
                }
            }
            'W' => {
                let t = leaves[(pos + leaves.len() - 1) % leaves.len()];
                self.focus_window(t);
            }
            'p' => {
                if let Some(p) = self.tab().prev.filter(|p| leaves.contains(p)) {
                    self.focus_window(p);
                }
            }
            't' => self.focus_window(leaves[0]),
            'b' => self.focus_window(*leaves.last().unwrap()),
            'h' | 'j' | 'k' | 'l' => {
                for _ in 0..n {
                    if let Some(w) = self.window_in_dir(c) {
                        self.focus_window(w);
                    }
                }
            }
            'q' => {
                let _ = self.run_ex("quit");
            }
            'c' => {
                let _ = self.run_ex("close");
            }
            'o' => self.only_window(),
            '=' => layout::equalize(&mut self.tab_mut().root),
            '+' | '-' | '>' | '<' => {
                let dir = if c == '+' || c == '-' { Dir::Horizontal } else { Dir::Vertical };
                let delta = if c == '+' || c == '>' { n as i32 } else { -(n as i32) };
                let area = self.editor_area();
                layout::resize(&mut self.tab_mut().root, area, cur, dir, delta);
            }
            '_' => layout::maximize(&mut self.tab_mut().root, cur, Dir::Horizontal),
            '|' => layout::maximize(&mut self.tab_mut().root, cur, Dir::Vertical),
            'T' => {
                if leaves.len() > 1 {
                    let buf = self.win().buf;
                    self.close_window(cur);
                    self.new_tab(buf);
                }
            }
            'x' | 'r'
                // Exchange with the next window: swap their buffers and views.
                if leaves.len() > 1 => {
                    let other = leaves[(pos + 1) % leaves.len()];
                    let a = self.windows.remove(&cur).unwrap();
                    let b = self.windows.remove(&other).unwrap();
                    let (mut a2, mut b2) = (b, a);
                    a2.id = cur;
                    b2.id = other;
                    self.windows.insert(cur, a2);
                    self.windows.insert(other, b2);
                    self.focus_window(other);
                }
            _ => {}
        }
        self.scroll_to_cursor();
    }
}
