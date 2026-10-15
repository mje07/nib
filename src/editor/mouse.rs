//! Mouse and bracketed-paste input.

use super::register::{RegKind, RegVal};
use super::{Editor, Mode, VKind};

impl Editor {
    pub fn mouse_down(&mut self, x: u16, y: u16) {
        if !self.options.mouse {
            return;
        }
        if self.tabs.len() > 1 && y == 0 {
            self.click_tabline(x);
            return;
        }
        if self.mode == Mode::Cmdline {
            return;
        }
        let Some((win, pos)) = self.screen_to_pos(x, y) else {
            // Statusline click focuses that window.
            let rects = super::layout::rects(&self.tab().root, self.editor_area());
            if let Some((id, _)) = rects.into_iter().find(|(_, r)| r.contains(x, y)) {
                self.focus_window(id);
            }
            return;
        };
        if self.mode.is_visual() {
            self.exit_visual();
        }
        self.pending.clear();
        self.focus_window(win);
        self.set_cursor_col(pos);
        self.mouse_drag = Some((win, self.cursor()));
        self.scroll_to_cursor();
    }

    pub fn mouse_drag_to(&mut self, x: u16, y: u16) {
        let Some((win, origin)) = self.mouse_drag else { return };
        let Some((w2, pos)) = self.screen_to_pos(x, y) else { return };
        if w2 != win || win != self.win_id() || pos == origin {
            return;
        }
        if !self.mode.is_visual() {
            if self.mode.is_insert() {
                self.leave_insert();
            }
            self.mode = Mode::Visual(VKind::Char);
            self.win_mut().vstart = origin;
        }
        self.win_mut().cursor = pos;
        self.clamp_cursor();
        self.scroll_to_cursor();
    }

    pub fn mouse_up(&mut self) {
        self.mouse_drag = None;
    }

    pub fn mouse_scroll(&mut self, x: u16, y: u16, down: bool) {
        let rects = super::layout::rects(&self.tab().root, self.editor_area());
        let Some((id, _)) = rects.into_iter().find(|(_, r)| r.contains(x, y)) else { return };
        let cur = self.win_id();
        self.tab_mut().active = id;
        self.scroll_lines(if down { 3 } else { -3 });
        self.tab_mut().active = cur;
    }

    fn click_tabline(&mut self, x: u16) {
        let mut pos = 0u16;
        for i in 0..self.tabs.len() {
            let t = &self.tabs[i];
            let win = &self.windows[&t.active];
            let b = &self.buffers[&win.buf];
            let name = b.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or("[No Name]".into());
            let w = (format!(" {} {} ", i + 1, name).chars().count() + 2) as u16;
            if x < pos + w {
                self.goto_tab(i);
                return;
            }
            pos += w;
        }
    }

    pub fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        match self.mode {
            Mode::Insert | Mode::Replace => {
                self.insert_text(&text);
            }
            Mode::Cmdline => {
                let first = text.lines().next().unwrap_or("").to_string();
                for c in first.chars() {
                    self.cmdline_key(crate::key::Key::ch(c));
                }
            }
            Mode::Normal | Mode::Visual(_) => {
                if self.mode.is_visual() {
                    self.exit_visual();
                }
                self.begin_change();
                let kind = if text.ends_with('\n') { RegKind::Line } else { RegKind::Char };
                let t = text.strip_suffix('\n').unwrap_or(&text).to_string();
                self.put_val(&RegVal::new(t, kind), kind == RegKind::Char, false, 1);
                self.commit();
            }
        }
        self.scroll_to_cursor();
    }
}
