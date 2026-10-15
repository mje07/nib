//! Highlight groups. Defaults use the terminal's ANSI palette so they follow
//! the user's terminal theme; any group can be overridden in config.toml.

use std::collections::HashMap;

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct Theme {
    groups: HashMap<String, Style>,
}

fn s() -> Style {
    Style::default()
}

impl Default for Theme {
    fn default() -> Self {
        use Color::*;
        let b = Modifier::BOLD;
        let i = Modifier::ITALIC;
        let u = Modifier::UNDERLINED;
        let d = Modifier::DIM;
        let code_bg = Indexed(236);
        // Obsidian's default accent purple.
        let accent = Rgb(167, 139, 250);
        let pairs: Vec<(&str, Style)> = vec![
            // UI
            ("normal", s()),
            ("linenr", s().fg(DarkGray)),
            ("cursorlinenr", s().fg(Yellow).add_modifier(b)),
            ("cursorline", s().bg(Indexed(235))),
            ("nontext", s().fg(DarkGray)),
            ("visual", s().bg(Indexed(239))),
            ("search", s().bg(Yellow).fg(Black)),
            ("incsearch", s().bg(LightRed).fg(Black)),
            ("statusline", s().bg(Indexed(238)).fg(White)),
            ("statuslinenc", s().bg(Indexed(236)).fg(Gray)),
            ("mode_normal", s().bg(Blue).fg(Black).add_modifier(b)),
            ("mode_insert", s().bg(Green).fg(Black).add_modifier(b)),
            ("mode_visual", s().bg(Magenta).fg(Black).add_modifier(b)),
            ("mode_replace", s().bg(Red).fg(Black).add_modifier(b)),
            ("mode_command", s().bg(Yellow).fg(Black).add_modifier(b)),
            ("tabline", s().bg(Indexed(236)).fg(Gray)),
            ("tablinesel", s().bg(Blue).fg(Black).add_modifier(b)),
            ("tablinefill", s().bg(Indexed(235))),
            ("vertsplit", s().fg(Indexed(238))),
            ("error", s().fg(LightRed).add_modifier(b)),
            ("warning", s().fg(Yellow)),
            ("moremsg", s().fg(Green).add_modifier(b)),
            ("fold", s().fg(Cyan).add_modifier(d)),
            ("pmenu", s().bg(Indexed(237))),
            ("pmenusel", s().bg(Blue).fg(Black)),
            ("matchparen", s().bg(Indexed(240)).add_modifier(b)),
            // Markdown
            // Obsidian's default theme: headings keep the text colour and are
            // set apart by size and weight; without real sizing, deeper
            // levels get progressively quieter.
            ("h1", s().fg(White).add_modifier(b)),
            ("h2", s().fg(White).add_modifier(b)),
            ("h3", s().add_modifier(b)),
            ("h4", s().add_modifier(b)),
            ("h5", s().fg(Gray).add_modifier(b)),
            ("h6", s().fg(DarkGray).add_modifier(b)),
            ("bold", s().add_modifier(b)),
            ("italic", s().add_modifier(i)),
            ("strike", s().add_modifier(Modifier::CROSSED_OUT).fg(Gray)),
            ("highlight", s().bg(Rgb(94, 84, 32))),
            ("code", s().bg(code_bg)),
            ("codeblock", s().bg(code_bg)),
            ("codefence", s().fg(DarkGray).bg(code_bg)),
            ("link", s().fg(accent).add_modifier(u)),
            ("url", s().fg(DarkGray).add_modifier(u)),
            ("image", s().fg(accent)),
            ("quote", s()),
            ("quotebar", s().fg(accent)),
            ("listmarker", s().fg(DarkGray)),
            ("taskbox", s().fg(accent)),
            ("taskdone", s().fg(DarkGray).add_modifier(Modifier::CROSSED_OUT)),
            ("tag", s().fg(accent).bg(Rgb(48, 41, 74))),
            ("hr", s().fg(DarkGray)),
            ("tableborder", s().fg(DarkGray)),
            ("tableheader", s().add_modifier(b)),
            ("markup", s().fg(DarkGray)),
            ("html", s().fg(DarkGray)),
            ("math", s().fg(LightGreen).add_modifier(i)),
            ("footnote", s().fg(accent)),
            ("frontmatter", s().fg(DarkGray)),
        ];
        Theme { groups: pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect() }
    }
}

impl Theme {
    pub fn get(&self, name: &str) -> Style {
        self.groups.get(name).copied().unwrap_or_default()
    }

    pub fn set(&mut self, name: &str, style: Style) {
        self.groups.insert(name.to_string(), style);
    }
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct StyleSpec {
    pub fg: Option<String>,
    pub bg: Option<String>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub dim: bool,
    #[serde(default)]
    pub reversed: bool,
    #[serde(default)]
    pub strikethrough: bool,
}

impl StyleSpec {
    pub fn to_style(&self) -> Result<Style, String> {
        let mut st = Style::default();
        if let Some(fg) = &self.fg {
            st = st.fg(parse_color(fg)?);
        }
        if let Some(bg) = &self.bg {
            st = st.bg(parse_color(bg)?);
        }
        let mut m = Modifier::empty();
        if self.bold {
            m |= Modifier::BOLD;
        }
        if self.italic {
            m |= Modifier::ITALIC;
        }
        if self.underline {
            m |= Modifier::UNDERLINED;
        }
        if self.dim {
            m |= Modifier::DIM;
        }
        if self.reversed {
            m |= Modifier::REVERSED;
        }
        if self.strikethrough {
            m |= Modifier::CROSSED_OUT;
        }
        Ok(st.add_modifier(m))
    }
}

pub fn parse_color(s: &str) -> Result<Color, String> {
    if let Ok(n) = s.parse::<u8>() {
        return Ok(Color::Indexed(n));
    }
    s.parse::<Color>().map_err(|_| format!("invalid color: {s}"))
}
