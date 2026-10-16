//! Highlight groups and colour schemes.
//!
//! The `default` scheme uses the terminal's ANSI palette so it follows the
//! user's terminal theme. Every other scheme works like an Obsidian theme: a
//! small palette of semantic colours (background, text, muted/faint text,
//! accent, heading colours, …) is mapped onto all highlight groups, and the
//! scheme paints its own background. Schemes are TOML files — the built-in ones
//! live in `themes/` and are compiled in; users can add their own (or override a
//! built-in) in `~/.config/mdvim/themes/NAME.toml`. Individual groups can still
//! be overridden in config.toml under `[theme]`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct Theme {
    /// Name the scheme was loaded under (`:colorscheme` shows it).
    pub name: String,
    /// Human-readable name from the scheme file.
    pub title: Option<String>,
    groups: HashMap<String, Style>,
    /// Resolved palette; `None` for the ANSI `default` scheme.
    palette: Option<Palette>,
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
        Theme {
            name: "default".into(),
            title: None,
            groups: pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            palette: None,
        }
    }
}

impl Theme {
    pub fn get(&self, name: &str) -> Style {
        self.groups.get(name).copied().unwrap_or_default()
    }

    pub fn set(&mut self, name: &str, style: Style) {
        self.groups.insert(name.to_string(), style);
    }

    /// Apply group overrides; colours may name palette entries. Returns errors
    /// as `group: message`.
    pub fn apply_specs<'a>(&mut self, specs: impl IntoIterator<Item = (&'a String, &'a StyleSpec)>) -> Vec<String> {
        let mut errs = Vec::new();
        for (group, spec) in specs {
            match spec.to_style(self.palette.as_ref()) {
                Ok(st) => self.set(group, st),
                Err(e) => errs.push(format!("{group}: {e}")),
            }
        }
        errs
    }

    /// Map a palette onto every highlight group, the way an Obsidian theme's
    /// CSS variables cascade into the whole interface.
    fn from_palette(name: &str, p: Palette) -> Theme {
        let b = Modifier::BOLD;
        let i = Modifier::ITALIC;
        let u = Modifier::UNDERLINED;
        let x = Modifier::CROSSED_OUT;
        let c = |k: &str| p.get(k);
        let bg = c("bg");
        let fg = c("fg");
        // Text drawn on a coloured block (mode badges, search hits).
        let badge = |k: &str| s().bg(c(k)).fg(bg).add_modifier(b);
        let pairs: Vec<(&str, Style)> = vec![
            // UI
            ("normal", s().fg(fg).bg(bg)),
            ("linenr", s().fg(c("fg_faint"))),
            ("cursorlinenr", s().fg(c("cursorlinenr")).add_modifier(b)),
            ("cursorline", s().bg(c("bg_hl"))),
            ("nontext", s().fg(c("fg_faint"))),
            ("visual", s().bg(c("selection"))),
            ("search", s().bg(c("yellow")).fg(bg)),
            ("incsearch", s().bg(c("orange")).fg(bg)),
            ("statusline", s().bg(c("statusline")).fg(fg)),
            ("statuslinenc", s().bg(c("bg_dark")).fg(c("fg_faint"))),
            ("mode_normal", badge("accent")),
            ("mode_insert", badge("green")),
            ("mode_visual", badge("purple")),
            ("mode_replace", badge("red")),
            ("mode_command", badge("yellow")),
            ("tabline", s().bg(c("bg_dark")).fg(c("fg_muted"))),
            ("tablinesel", badge("accent")),
            ("tablinefill", s().bg(c("bg_dark"))),
            ("vertsplit", s().fg(c("border"))),
            ("error", s().fg(c("red")).add_modifier(b)),
            ("warning", s().fg(c("yellow"))),
            ("moremsg", s().fg(c("green")).add_modifier(b)),
            ("fold", s().fg(c("cyan"))),
            ("pmenu", s().bg(c("bg_alt")).fg(fg)),
            ("pmenusel", s().bg(c("selection")).fg(fg).add_modifier(b)),
            ("matchparen", s().bg(c("selection")).add_modifier(b)),
            // Markdown
            ("h1", s().fg(c("h1")).add_modifier(b)),
            ("h2", s().fg(c("h2")).add_modifier(b)),
            ("h3", s().fg(c("h3")).add_modifier(b)),
            ("h4", s().fg(c("h4")).add_modifier(b)),
            ("h5", s().fg(c("h5")).add_modifier(b)),
            ("h6", s().fg(c("h6")).add_modifier(b)),
            ("bold", s().fg(c("bold")).add_modifier(b)),
            ("italic", s().fg(c("italic")).add_modifier(i)),
            ("strike", s().fg(c("fg_muted")).add_modifier(x)),
            ("highlight", s().bg(c("highlight_bg"))),
            ("code", s().fg(c("code")).bg(c("bg_alt"))),
            ("codeblock", s().bg(c("bg_alt"))),
            ("codefence", s().fg(c("fg_faint")).bg(c("bg_alt"))),
            ("link", s().fg(c("link")).add_modifier(u)),
            ("url", s().fg(c("fg_faint")).add_modifier(u)),
            ("image", s().fg(c("link"))),
            ("quote", s().fg(c("quote"))),
            ("quotebar", s().fg(c("accent"))),
            ("listmarker", s().fg(c("listmarker"))),
            ("taskbox", s().fg(c("accent"))),
            ("taskdone", s().fg(c("fg_faint")).add_modifier(x)),
            ("tag", s().fg(c("tag")).bg(c("tag_bg"))),
            ("hr", s().fg(c("border"))),
            ("tableborder", s().fg(c("border"))),
            ("tableheader", s().fg(fg).add_modifier(b)),
            ("markup", s().fg(c("fg_faint"))),
            ("html", s().fg(c("fg_faint"))),
            ("math", s().fg(c("math")).add_modifier(i)),
            ("footnote", s().fg(c("accent"))),
            ("frontmatter", s().fg(c("fg_faint"))),
        ];
        Theme {
            name: name.to_string(),
            title: None,
            groups: pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            palette: Some(p),
        }
    }
}

// --- Palettes ------------------------------------------------------------------

/// How a palette entry is derived when a scheme leaves it out.
enum Fallback {
    Required,
    Key(&'static str),
    Ansi(Color),
    /// `a` mixed into `b` by the given amount, like Obsidian's translucent
    /// `rgba(accent, 0.15)` backgrounds; RGB colours only, else the last key.
    Blend(&'static str, &'static str, f32, &'static str),
}

/// Palette entries in resolution order (fallbacks only refer to earlier keys).
const PALETTE: &[(&str, Fallback)] = {
    use Fallback::*;
    &[
        ("bg", Required),           // editor background
        ("fg", Required),           // normal text
        ("accent", Required),       // Obsidian's interactive accent
        ("bg_dark", Key("bg")),     // inactive statusline, tabline
        ("bg_alt", Key("bg_dark")), // code, completion menu
        ("bg_hl", Key("bg_alt")),   // cursorline
        ("selection", Key("bg_hl")),
        ("statusline", Key("bg_alt")),
        ("fg_muted", Key("fg")),
        ("fg_faint", Key("fg_muted")), // line numbers, markup, front matter
        ("border", Key("fg_faint")),   // splits, table borders, rules
        ("red", Ansi(Color::Red)),
        ("orange", Key("red")),
        ("yellow", Ansi(Color::Yellow)),
        ("green", Ansi(Color::Green)),
        ("cyan", Ansi(Color::Cyan)),
        ("blue", Ansi(Color::Blue)),
        ("purple", Ansi(Color::Magenta)),
        ("h1", Key("fg")),
        ("h2", Key("fg")),
        ("h3", Key("fg")),
        ("h4", Key("fg")),
        ("h5", Key("fg")),
        ("h6", Key("fg")),
        ("bold", Key("fg")),
        ("italic", Key("fg")),
        ("code", Key("fg")),
        ("link", Key("accent")),
        ("tag", Key("accent")),
        ("tag_bg", Blend("tag", "bg", 0.15, "bg_alt")),
        ("quote", Key("fg")),
        ("listmarker", Key("fg_faint")),
        ("highlight_bg", Blend("yellow", "bg", 0.3, "selection")),
        ("math", Key("cyan")),
        ("cursorlinenr", Key("accent")),
    ]
};

#[derive(Clone, Debug)]
pub struct Palette {
    colors: HashMap<String, Color>,
}

impl Palette {
    fn resolve(raw: &HashMap<String, String>) -> Result<Palette, String> {
        let mut unknown: Vec<&String> = raw.keys().filter(|k| !PALETTE.iter().any(|(n, _)| n == k)).collect();
        if !unknown.is_empty() {
            unknown.sort();
            return Err(format!("unknown palette key {}", unknown[0]));
        }
        let mut colors = HashMap::new();
        for (key, fb) in PALETTE {
            let col = match (raw.get(*key), fb) {
                (Some(v), _) => parse_color(v).map_err(|e| format!("palette.{key}: {e}"))?,
                (None, Fallback::Required) => return Err(format!("palette.{key} is required")),
                (None, Fallback::Key(k)) => colors[*k],
                (None, Fallback::Ansi(c)) => *c,
                (None, Fallback::Blend(a, b, t, other)) => blend(colors[*a], colors[*b], *t).unwrap_or(colors[*other]),
            };
            colors.insert(key.to_string(), col);
        }
        Ok(Palette { colors })
    }

    fn get(&self, key: &str) -> Color {
        self.colors[key]
    }
}

fn blend(a: Color, b: Color, t: f32) -> Option<Color> {
    let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (a, b) else { return None };
    let mix = |x: u8, y: u8| (x as f32 * t + y as f32 * (1.0 - t)).round() as u8;
    Some(Color::Rgb(mix(r1, r2), mix(g1, g2), mix(b1, b2)))
}

// --- Scheme files --------------------------------------------------------------

/// A colour-scheme file (`themes/*.toml`).
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    /// Human-readable name (informational).
    pub name: Option<String>,
    /// Scheme to start from; this file's palette and groups are layered on top.
    pub base: Option<String>,
    #[serde(default)]
    pub palette: HashMap<String, String>,
    /// Per-group overrides, applied after the palette mapping.
    #[serde(default)]
    pub groups: HashMap<String, StyleSpec>,
}

macro_rules! builtin {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../themes/", $name, ".toml")))),*]
    };
}

/// Built-in schemes, compiled in from `themes/`.
const BUILTIN: &[(&str, &str)] = builtin!(
    "obsidian",
    "obsidian-light",
    "kanagawa",
    "kanagawa-dragon",
    "kanagawa-lotus",
    "tokyonight",
    "tokyonight-storm",
    "tokyonight-moon",
    "tokyonight-day",
    "catppuccin-mocha",
    "catppuccin-macchiato",
    "catppuccin-frappe",
    "catppuccin-latte",
    "rose-pine",
    "rose-pine-moon",
    "rose-pine-dawn",
    "gruvbox",
    "gruvbox-light",
    "nord",
    "dracula",
    "everforest",
);

const ALIASES: &[(&str, &str)] = &[
    ("catppuccin", "catppuccin-mocha"),
    ("kanagawa-wave", "kanagawa"),
    ("tokyonight-night", "tokyonight"),
    ("tokyo-night", "tokyonight"),
    ("rosepine", "rose-pine"),
    ("gruvbox-dark", "gruvbox"),
];

/// Directory for user colour schemes: `~/.config/mdvim/themes`.
pub fn user_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("mdvim").join("themes"))
}

/// Where `:colorscheme` remembers the last choice: `~/.local/state/mdvim/colorscheme`.
pub fn saved_path() -> Option<PathBuf> {
    dirs::state_dir().or_else(dirs::data_dir).map(|d| d.join("mdvim").join("colorscheme"))
}

/// The remembered scheme and when it was chosen.
pub fn read_saved(path: &Path) -> Option<(String, std::time::SystemTime)> {
    let name = std::fs::read_to_string(path).ok()?.trim().to_string();
    let when = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    (!name.is_empty()).then_some((name, when))
}

pub fn save(path: &Path, name: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, format!("{name}\n"))
}

/// Every scheme name `:colorscheme` accepts (built-in and user), sorted.
pub fn available(user_dir: Option<&Path>) -> Vec<String> {
    let mut names: Vec<String> = std::iter::once("default").chain(BUILTIN.iter().map(|(n, _)| *n)).map(String::from).collect();
    if let Some(entries) = user_dir.and_then(|d| std::fs::read_dir(d).ok()) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "toml")
                && let Some(stem) = p.file_stem()
            {
                names.push(stem.to_string_lossy().into_owned());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Source text of a scheme. User files win over built-ins of the same name,
/// unless `allow_user` is false (a user scheme extending the built-in it shadows).
fn source(name: &str, user_dir: Option<&Path>, allow_user: bool) -> Result<Option<String>, String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    if name.ends_with(".toml") || name.contains('/') {
        return read(&crate::editor::ex::expand_path(name)).map(Some);
    }
    if allow_user && let Some(dir) = user_dir {
        let p = dir.join(format!("{name}.toml"));
        if p.is_file() {
            return read(&p).map(Some);
        }
    }
    if name == "default" {
        return Ok(None);
    }
    let name = ALIASES.iter().find(|(a, _)| *a == name).map(|(_, n)| *n).unwrap_or(name);
    match BUILTIN.iter().find(|(n, _)| *n == name) {
        Some((_, text)) => Ok(Some(text.to_string())),
        None => Err(format!("E185: Cannot find color scheme '{name}'")),
    }
}

/// Collect palette entries and group overrides along the `base` chain;
/// returns the scheme's title.
fn collect(
    name: &str,
    user_dir: Option<&Path>,
    allow_user: bool,
    depth: usize,
    palette: &mut HashMap<String, String>,
    groups: &mut Vec<(String, StyleSpec)>,
) -> Result<Option<String>, String> {
    if depth > 8 {
        return Err(format!("color scheme '{name}': base chain too deep"));
    }
    let Some(text) = source(name, user_dir, allow_user)? else { return Ok(None) };
    let file: ThemeFile = toml::from_str(&text).map_err(|e| format!("color scheme '{name}': {}", e.message()))?;
    if let Some(base) = &file.base {
        collect(base, user_dir, base != name, depth + 1, palette, groups)?;
    }
    palette.extend(file.palette);
    groups.extend(file.groups);
    Ok(file.name)
}

/// Load a colour scheme by name (or by path to a `.toml` file).
pub fn load(name: &str, user_dir: Option<&Path>) -> Result<Theme, String> {
    let mut palette = HashMap::new();
    let mut groups = Vec::new();
    let title = collect(name, user_dir, true, 0, &mut palette, &mut groups)?;
    let mut theme = if palette.is_empty() {
        Theme { name: name.to_string(), ..Theme::default() }
    } else {
        let p = Palette::resolve(&palette).map_err(|e| format!("color scheme '{name}': {e}"))?;
        Theme::from_palette(name, p)
    };
    theme.title = title;
    let errs = theme.apply_specs(groups.iter().map(|(g, s)| (g, s)));
    match errs.first() {
        Some(e) => Err(format!("color scheme '{name}': groups.{e}")),
        None => Ok(theme),
    }
}

// --- Style specs ---------------------------------------------------------------

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
    /// Colours may be palette entry names ("accent", "h1", "red", …), which
    /// take precedence over terminal colour names when a palette is active.
    pub fn to_style(&self, palette: Option<&Palette>) -> Result<Style, String> {
        let color = |v: &str| match palette.and_then(|p| p.colors.get(v)) {
            Some(c) => Ok(*c),
            None => parse_color(v),
        };
        let mut st = Style::default();
        if let Some(fg) = &self.fg {
            st = st.fg(color(fg)?);
        }
        if let Some(bg) = &self.bg {
            st = st.bg(color(bg)?);
        }
        let mut m = Modifier::empty();
        for (on, bit) in [
            (self.bold, Modifier::BOLD),
            (self.italic, Modifier::ITALIC),
            (self.underline, Modifier::UNDERLINED),
            (self.dim, Modifier::DIM),
            (self.reversed, Modifier::REVERSED),
            (self.strikethrough, Modifier::CROSSED_OUT),
        ] {
            if on {
                m |= bit;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_load() {
        for name in available(None) {
            let t = load(&name, None).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(t.name, name);
            if name != "default" {
                // Palette schemes paint their own background.
                assert!(t.get("normal").bg.is_some(), "{name}");
            }
        }
        for (alias, _) in ALIASES {
            load(alias, None).unwrap();
        }
    }

    #[test]
    fn unknown_scheme_errors() {
        assert!(load("nope", None).unwrap_err().contains("E185"));
    }

    #[test]
    fn user_scheme_extends_builtin() {
        let dir = std::env::temp_dir().join(format!("mdvim-theme-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Shadows the built-in of the same name and extends it.
        std::fs::write(
            dir.join("nord.toml"),
            "base = \"nord\"\n[palette]\naccent = \"#ff0000\"\n[groups]\nh1 = { fg = \"accent\", italic = true }\n",
        )
        .unwrap();
        let t = load("nord", Some(&dir)).unwrap();
        assert_eq!(t.get("quotebar").fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(t.get("h1").fg, Some(Color::Rgb(255, 0, 0)));
        assert!(t.get("h1").add_modifier.contains(Modifier::ITALIC));
        // Background still comes from the built-in nord palette.
        assert_eq!(t.get("normal").bg, Some(Color::Rgb(0x2e, 0x34, 0x40)));
        assert!(available(Some(&dir)).contains(&"nord".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn palette_fallbacks_and_blend() {
        let raw: HashMap<String, String> =
            [("bg", "#000000"), ("fg", "#ffffff"), ("accent", "#c8c8c8")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let p = Palette::resolve(&raw).unwrap();
        assert_eq!(p.get("h3"), Color::Rgb(255, 255, 255));
        assert_eq!(p.get("link"), Color::Rgb(200, 200, 200));
        assert_eq!(p.get("tag_bg"), Color::Rgb(30, 30, 30));
        let mut bad = raw.clone();
        bad.insert("acent".into(), "#000000".into());
        assert!(Palette::resolve(&bad).unwrap_err().contains("acent"));
    }
}
