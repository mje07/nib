//! Large headings drawn as images with the kitty graphics protocol, for
//! terminals (Ghostty, WezTerm, …) that support graphics but not the kitty
//! text-sizing protocol.
//!
//! Each scaled heading overlay is rasterised with the terminal's own font —
//! read from Ghostty's `font-family` settings — at the heading's size, in the
//! heading's colour, uploaded once and placed over the blank cells the layout
//! reserved for it.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::style::{Color, Modifier};
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;
use swash::{FontRef, NormalizedCoord};

use crate::ui::TextOverlay;

/// Ghostty's built-in default font, used when Ghostty has no `font-family`
/// (or it can't be found), so headings match what Ghostty actually draws.
const EMBEDDED_FONT: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf");

/// Uploaded images kept before the cache is flushed.
const MAX_IMAGES: usize = 128;

// --- Terminal capabilities -------------------------------------------------------

/// What the startup probe learnt about the terminal.
#[derive(Debug, Default, PartialEq)]
pub struct Probe {
    pub graphics: bool,
    /// Default foreground (OSC 10).
    pub fg: Option<[u8; 3]>,
    /// ANSI palette entries 0–15 (OSC 4).
    pub palette: HashMap<u8, [u8; 3]>,
    /// Cell size in pixels (CSI 16 t), when the ioctl doesn't report it.
    pub cell: Option<(u16, u16)>,
}

/// Ask the terminal for graphics support, its colours and cell size. Must run
/// in raw mode before the event loop starts reading input.
pub fn probe() -> Probe {
    let mut out = io::stdout();
    let mut q = String::from("\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b]10;?\x1b\\\x1b[16t");
    for i in 0..16 {
        q.push_str(&format!("\x1b]4;{i};?\x1b\\"));
    }
    // Primary device attributes: every terminal answers, so its reply marks
    // the end of the responses.
    q.push_str("\x1b[c");
    if out.write_all(q.as_bytes()).and_then(|_| out.flush()).is_err() {
        return Probe::default();
    }
    parse_probe(&read_until_da1(Duration::from_millis(500)))
}

fn read_until_da1(timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    let mut buf = Vec::new();
    let da1 = regex::bytes::Regex::new(r"\x1b\[\?[0-9;]*c").unwrap();
    while !da1.is_match(&buf) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        // SAFETY: polling and reading stdin with valid, owned buffers.
        let ready = unsafe { libc::poll(&mut pfd, 1, left.as_millis().max(1) as i32) };
        if ready <= 0 {
            break;
        }
        let mut chunk = [0u8; 4096];
        let n = unsafe { libc::read(0, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n <= 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n as usize]);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn parse_probe(s: &str) -> Probe {
    let mut p = Probe { graphics: s.contains("\x1b_Gi=31;OK"), ..Default::default() };
    let rgb = r"rgb:([0-9a-fA-F]{1,4})/([0-9a-fA-F]{1,4})/([0-9a-fA-F]{1,4})";
    let col = |c: &regex::Captures, i: usize| -> [u8; 3] {
        let ch = |m: &str| {
            let v = u32::from_str_radix(m, 16).unwrap_or(0);
            let max = (1u32 << (4 * m.len())) - 1;
            (v * 255 / max) as u8
        };
        [ch(&c[i]), ch(&c[i + 1]), ch(&c[i + 2])]
    };
    if let Some(c) = regex::Regex::new(&format!(r"\x1b\]10;{rgb}")).unwrap().captures(s) {
        p.fg = Some(col(&c, 1));
    }
    for c in regex::Regex::new(&format!(r"\x1b\]4;(\d+);{rgb}")).unwrap().captures_iter(s) {
        if let Ok(i) = c[1].parse::<u8>() {
            p.palette.insert(i, col(&c, 2));
        }
    }
    if let Some(c) = regex::Regex::new(r"\x1b\[6;(\d+);(\d+)t").unwrap().captures(s)
        && let (Ok(h), Ok(w)) = (c[1].parse(), c[2].parse())
    {
        p.cell = Some((w, h));
    }
    p
}

/// Cell size in pixels from the tty's window size, if reported.
fn ioctl_cell_size() -> Option<(u16, u16)> {
    // SAFETY: TIOCGWINSZ fills a winsize struct we own.
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } != 0 {
        return None;
    }
    if ws.ws_xpixel == 0 || ws.ws_ypixel == 0 || ws.ws_col == 0 || ws.ws_row == 0 {
        return None;
    }
    Some((ws.ws_xpixel / ws.ws_col, ws.ws_ypixel / ws.ws_row))
}

// --- Font selection --------------------------------------------------------------

/// Font settings from Ghostty's configuration.
#[derive(Debug, Default, PartialEq)]
pub struct GhosttyFonts {
    pub family: Option<String>,
    pub family_bold: Option<String>,
    pub style_bold: Option<String>,
}

pub fn in_ghostty() -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|v| v.eq_ignore_ascii_case("ghostty"))
        || std::env::var("TERM").is_ok_and(|v| v == "xterm-ghostty")
}

/// Ghostty's effective font settings: from `ghostty +show-config`, which
/// resolves includes and defaults, else by reading the config files.
pub fn ghostty_fonts() -> GhosttyFonts {
    if let Ok(out) = Command::new("ghostty").arg("+show-config").output()
        && out.status.success()
    {
        return parse_ghostty_config(&String::from_utf8_lossy(&out.stdout), None, 0);
    }
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|d| d.join("ghostty"));
    let mut text = String::new();
    let mut base = None;
    let mut candidates: Vec<PathBuf> = dir.iter().flat_map(|d| [d.join("config"), d.join("config.ghostty")]).collect();
    if let Some(h) = dirs::home_dir() {
        candidates.push(h.join("Library/Application Support/com.mitchellh.ghostty/config"));
    }
    for p in candidates {
        if let Ok(t) = std::fs::read_to_string(&p) {
            text.push_str(&t);
            text.push('\n');
            base = p.parent().map(Path::to_path_buf);
        }
    }
    parse_ghostty_config(&text, base.as_deref(), 0)
}

fn parse_ghostty_config(text: &str, base: Option<&Path>, depth: usize) -> GhosttyFonts {
    let mut f = GhosttyFonts::default();
    let merge = |f: &mut GhosttyFonts, other: GhosttyFonts| {
        f.family = other.family.or(f.family.take());
        f.family_bold = other.family_bold.or(f.family_bold.take());
        f.style_bold = other.style_bold.or(f.style_bold.take());
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().trim_matches('"').to_string();
        let set = |slot: &mut Option<String>, first_only: bool| {
            if v.is_empty() {
                // An empty value resets the setting.
                *slot = None;
            } else if !(first_only && slot.is_some()) {
                *slot = Some(v.clone());
            }
        };
        match k.trim() {
            // Repeated font-family entries are fallbacks; the first is primary.
            "font-family" => set(&mut f.family, true),
            "font-family-bold" => set(&mut f.family_bold, true),
            "font-style-bold" if v != "default" => set(&mut f.style_bold, false),
            "config-file" if depth < 8 && !v.is_empty() => {
                let path = v.trim_start_matches('?');
                let path = match (path.strip_prefix("~/"), base) {
                    (Some(rest), _) => dirs::home_dir().map(|h| h.join(rest)),
                    (None, Some(b)) => Some(b.join(path)),
                    (None, None) => Some(PathBuf::from(path)),
                };
                if let Some(t) = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
                    let inner = parse_ghostty_config(&t, path.as_deref().and_then(Path::parent), depth + 1);
                    merge(&mut f, inner);
                }
            }
            _ => {}
        }
    }
    f
}

/// A font file (owned or embedded) and a face index within it.
struct FontData {
    data: Vec<u8>,
    /// Face index in a collection.
    index: usize,
}

impl FontData {
    fn font(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(&self.data, self.index)
    }
}

/// `fc-match` a pattern: (file, face index, family).
fn fc_match(pattern: &str) -> Option<(PathBuf, usize, String)> {
    let out = Command::new("fc-match").args(["-f", "%{file}\n%{index}\n%{family}", pattern]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut it = text.lines();
    let file = PathBuf::from(it.next()?);
    // fontconfig encodes a variable font's named instance in the high bits.
    let index = it.next()?.parse::<usize>().ok()? & 0xffff;
    let family = it.next().unwrap_or("").to_string();
    Some((file, index, family))
}

fn load_file(path: &Path, index: usize) -> Option<FontData> {
    let fd = FontData { data: std::fs::read(path).ok()?, index };
    fd.font()?;
    Some(fd)
}

fn embedded() -> FontData {
    FontData { data: EMBEDDED_FONT.to_vec(), index: 0 }
}

/// The bold face of the terminal's font.
fn primary_font() -> FontData {
    let ghostty = in_ghostty();
    let cfg = if ghostty { ghostty_fonts() } else { GhosttyFonts::default() };
    let family = cfg.family_bold.as_ref().or(cfg.family.as_ref());
    let style = match &cfg.style_bold {
        Some(s) => format!(":style={s}"),
        None => ":weight=bold".into(),
    };
    match family {
        Some(fam) => {
            // fontconfig substitutes something else for a missing family;
            // Ghostty then uses its built-in font, and so do we.
            if let Some((file, idx, got)) = fc_match(&format!("{fam}{style}"))
                && got.split(',').any(|g| g.trim().eq_ignore_ascii_case(fam))
                && let Some(fd) = load_file(&file, idx)
            {
                return fd;
            }
            embedded()
        }
        None if ghostty => embedded(),
        None => fc_match(&format!("monospace{style}")).and_then(|(f, i, _)| load_file(&f, i)).unwrap_or_else(embedded),
    }
}

/// A loaded face with bold variation coordinates.
struct Face {
    data: FontData,
    coords: Vec<NormalizedCoord>,
}

impl Face {
    fn new(data: FontData) -> Option<Face> {
        let font = data.font()?;
        // Variable fonts: ask for bold explicitly.
        let coords = match font.variations().find_by_tag(swash::tag_from_str_lossy("wght")) {
            Some(v) => {
                let w = 700f32.clamp(v.min_value(), v.max_value());
                font.variations().normalized_coords([("wght", w)]).collect()
            }
            None => Vec::new(),
        };
        Some(Face { data, coords })
    }
}

// --- Rendering ---------------------------------------------------------------------

#[derive(Clone, PartialEq, Eq, Hash)]
struct ImageKey {
    text: Vec<(char, u8)>,
    scale: (u8, u8, u8),
    fg: [u8; 3],
    bg: Option<[u8; 3]>,
    modifiers: u16,
}

/// An RGBA image.
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct HeadingImages {
    faces: Vec<Face>,
    fallback: HashMap<char, Option<usize>>,
    ctx: ScaleContext,
    cell: (u16, u16),
    probe_cell: Option<(u16, u16)>,
    fg: [u8; 3],
    palette: HashMap<u8, [u8; 3]>,
    cache: HashMap<ImageKey, u32>,
    /// Set after a resize: the terminal may have dropped every image.
    stale: bool,
    next_id: u32,
}

impl HeadingImages {
    /// Set up image headings if the terminal supports kitty graphics.
    pub fn detect() -> Option<HeadingImages> {
        if std::env::var_os("NIB_NO_HEADING_IMAGES").is_some() {
            return None;
        }
        let p = probe();
        if !p.graphics {
            return None;
        }
        let cell = ioctl_cell_size().or(p.cell)?;
        let mut h = HeadingImages::new(Face::new(primary_font())?, cell);
        h.probe_cell = p.cell;
        h.fg = p.fg.unwrap_or(h.fg);
        h.palette.extend(p.palette);
        Some(h)
    }

    fn new(face: Face, cell: (u16, u16)) -> HeadingImages {
        HeadingImages {
            faces: vec![face],
            fallback: HashMap::new(),
            ctx: ScaleContext::new(),
            cell,
            probe_cell: None,
            fg: [0xdd, 0xdd, 0xdd],
            palette: HashMap::new(),
            cache: HashMap::new(),
            next_id: 1,
            stale: false,
        }
    }

    /// Escape sequences replacing all heading images on screen with `overlays`.
    /// Uploads images not seen before.
    pub fn draw(&mut self, overlays: &[TextOverlay]) -> String {
        // The font size may have changed (Ghostty's ctrl+=): re-rasterise.
        let cell = ioctl_cell_size().or(self.probe_cell).unwrap_or(self.cell);
        let mut out = String::new();
        if self.stale || cell != self.cell || self.cache.len() > MAX_IMAGES {
            self.stale = false;
            self.cell = cell;
            self.cache.clear();
            out.push_str("\x1b_Ga=d,d=A,q=2\x1b\\");
        } else {
            // Remove placements, keep the uploaded images.
            out.push_str("\x1b_Ga=d,d=a,q=2\x1b\\");
        }
        for o in overlays {
            let key = self.key(o);
            let id = match self.cache.get(&key) {
                Some(id) => *id,
                None => {
                    let Some(r) = self.rasterize(o, &key) else { continue };
                    let id = self.next_id;
                    self.next_id += 1;
                    transmit(&mut out, id, &r);
                    self.cache.insert(key.clone(), id);
                    id
                }
            };
            let (cols, rows) = self.block(o);
            out.push_str(&format!("\x1b[{};{}H\x1b_Ga=p,i={id},c={cols},r={rows},C=1,q=2\x1b\\", o.y + 1, o.x + 1));
        }
        out
    }

    /// The window was resized: the screen gets cleared, which drops image
    /// placements (and in some terminals the images), so start over.
    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    /// Remove every image (on exit or suspend).
    pub fn clear() -> &'static str {
        "\x1b_Ga=d,d=A,q=2\x1b\\"
    }

    fn rgb(&self, c: Option<Color>) -> Option<[u8; 3]> {
        let idx = |i: u8| self.palette.get(&i).copied().unwrap_or_else(|| xterm_color(i));
        Some(match c? {
            Color::Reset => return None,
            Color::Rgb(r, g, b) => [r, g, b],
            Color::Indexed(i) => idx(i),
            Color::Black => idx(0),
            Color::Red => idx(1),
            Color::Green => idx(2),
            Color::Yellow => idx(3),
            Color::Blue => idx(4),
            Color::Magenta => idx(5),
            Color::Cyan => idx(6),
            Color::Gray => idx(7),
            Color::DarkGray => idx(8),
            Color::LightRed => idx(9),
            Color::LightGreen => idx(10),
            Color::LightYellow => idx(11),
            Color::LightBlue => idx(12),
            Color::LightMagenta => idx(13),
            Color::LightCyan => idx(14),
            Color::White => idx(15),
        })
    }

    fn key(&self, o: &TextOverlay) -> ImageKey {
        ImageKey {
            text: o.chars.clone(),
            scale: (o.scale.s, o.scale.n, o.scale.d),
            fg: self.rgb(o.style.fg).unwrap_or(self.fg),
            bg: self.rgb(o.style.bg),
            modifiers: o.style.add_modifier.bits(),
        }
    }

    /// Cells covered by an overlay's image: as many columns as the scaled
    /// text needs (within the overlay), `s` rows.
    fn block(&self, o: &TextOverlay) -> (u16, u16) {
        let base: usize = o.chars.iter().map(|(_, w)| *w as usize).sum();
        let cols = ((base as f32 * o.scale.factor()).ceil() as u16).clamp(1, o.width.max(1));
        (cols, o.scale.s as u16)
    }

    /// Face index able to draw `ch` (the primary face, else a fontconfig fallback).
    fn face_for(&mut self, ch: char) -> Option<usize> {
        let has = |f: &Face| f.data.font().is_some_and(|font| font.charmap().map(ch) != 0);
        if has(&self.faces[0]) {
            return Some(0);
        }
        if let Some(r) = self.fallback.get(&ch) {
            return *r;
        }
        if let Some(i) = self.faces.iter().position(has) {
            self.fallback.insert(ch, Some(i));
            return Some(i);
        }
        let found = fc_match(&format!(":weight=bold:charset={:x}", ch as u32)).and_then(|(file, idx, _)| {
            let face = Face::new(load_file(&file, idx)?)?;
            has(&face).then(|| {
                self.faces.push(face);
                self.faces.len() - 1
            })
        });
        self.fallback.insert(ch, found);
        found
    }

    fn rasterize(&mut self, o: &TextOverlay, key: &ImageKey) -> Option<Raster> {
        let cw = self.cell.0 as f32;
        let factor = o.scale.factor();
        let (cols, rows) = self.block(o);
        let (w, h) = (cols as u32 * self.cell.0 as u32, rows as u32 * self.cell.1 as u32);
        let mut img = vec![0u8; (w * h * 4) as usize];
        if let Some([r, g, b]) = key.bg {
            for px in img.as_chunks_mut::<4>().0 {
                *px = [r, g, b, 255];
            }
        }
        // Size the font so one normal cell holds one advance of the primary
        // (monospace) face, i.e. the terminal's own font size, then scale.
        let primary = &self.faces[0];
        let font = primary.data.font()?;
        let upem = font.metrics(&primary.coords).units_per_em as f32;
        let adv = font.glyph_metrics(&primary.coords).advance_width(font.charmap().map('0')).max(1.0);
        let ppem = cw / (adv / upem) * factor;
        let m = font.metrics(&primary.coords).scale(ppem);
        let line_h = m.ascent + m.descent;
        let baseline = ((h as f32 - line_h) / 2.0 + m.ascent).round();
        let slot = cw * factor;
        let fg = key.fg;

        let mut x = 0f32;
        for &(ch, cwidth) in &o.chars {
            let slot_w = slot * cwidth as f32;
            if !ch.is_whitespace()
                && let Some(fi) = self.face_for(ch)
            {
                let face = &self.faces[fi];
                let font = face.data.font()?;
                let gid = font.charmap().map(ch);
                let gm = font.glyph_metrics(&face.coords).scale(ppem);
                let mut scaler = self.ctx.builder(font).size(ppem).hint(true).normalized_coords(&face.coords).build();
                let glyph = Render::new(&[
                    Source::ColorOutline(0),
                    Source::ColorBitmap(StrikeWith::BestFit),
                    Source::Outline,
                ])
                .format(Format::Alpha)
                .render(&mut scaler, gid);
                if let Some(g) = glyph {
                    let gx = x + (slot_w - gm.advance_width(gid)).max(0.0) / 2.0;
                    if matches!(g.source, Source::ColorBitmap(_)) {
                        // Bitmap emoji come at a fixed size: fit them in the slot.
                        let fit = (slot_w / g.placement.width as f32).min(line_h / g.placement.height as f32);
                        let (tw, th) = ((g.placement.width as f32 * fit) as u32, (g.placement.height as f32 * fit) as u32);
                        let top = baseline - m.ascent + (line_h - th as f32) / 2.0;
                        let left = x + (slot_w - tw as f32) / 2.0;
                        let scaled = resize_rgba(&g.data, g.placement.width, g.placement.height, tw, th);
                        blit(&mut img, w, h, left as i32, top as i32, tw, th, |i| {
                            let p = &scaled[i * 4..i * 4 + 4];
                            [p[0], p[1], p[2], p[3]]
                        });
                    } else {
                        let (gw, gh) = (g.placement.width, g.placement.height);
                        let left = (gx + g.placement.left as f32).round() as i32;
                        let top = (baseline - g.placement.top as f32) as i32;
                        match g.content {
                            Content::Color => blit(&mut img, w, h, left, top, gw, gh, |i| {
                                let p = &g.data[i * 4..i * 4 + 4];
                                [p[0], p[1], p[2], p[3]]
                            }),
                            _ => blit(&mut img, w, h, left, top, gw, gh, |i| [fg[0], fg[1], fg[2], g.data[i]]),
                        }
                    }
                }
            }
            x += slot_w;
        }
        let mods = Modifier::from_bits_truncate(key.modifiers);
        let thick = (ppem / 14.0).round().max(1.0) as u32;
        let text_w = x.min(w as f32) as u32;
        if mods.contains(Modifier::UNDERLINED) {
            let y = (baseline + m.underline_offset.abs().max(thick as f32)) as i32;
            blit(&mut img, w, h, 0, y, text_w, thick, |_| [fg[0], fg[1], fg[2], 255]);
        }
        if mods.contains(Modifier::CROSSED_OUT) {
            let y = (baseline - m.x_height / 2.0) as i32;
            blit(&mut img, w, h, 0, y, text_w, thick, |_| [fg[0], fg[1], fg[2], 255]);
        }
        Some(Raster { width: w, height: h, rgba: img })
    }
}

/// Composite a `gw`×`gh` straight-alpha source over the image at (x, y).
#[allow(clippy::too_many_arguments)]
fn blit(img: &mut [u8], w: u32, h: u32, x: i32, y: i32, gw: u32, gh: u32, src: impl Fn(usize) -> [u8; 4]) {
    for gy in 0..gh {
        let py = y + gy as i32;
        if py < 0 || py >= h as i32 {
            continue;
        }
        for gx in 0..gw {
            let px = x + gx as i32;
            if px < 0 || px >= w as i32 {
                continue;
            }
            let s = src((gy * gw + gx) as usize);
            let sa = s[3] as f32 / 255.0;
            if sa == 0.0 {
                continue;
            }
            let o = ((py as u32 * w + px as u32) * 4) as usize;
            let d = &mut img[o..o + 4];
            let da = d[3] as f32 / 255.0;
            let oa = sa + da * (1.0 - sa);
            for c in 0..3 {
                d[c] = ((s[c] as f32 * sa + d[c] as f32 * da * (1.0 - sa)) / oa).round() as u8;
            }
            d[3] = (oa * 255.0).round() as u8;
        }
    }
}

/// Box-filter downscale (or nearest upscale) of an RGBA bitmap.
fn resize_rgba(src: &[u8], sw: u32, sh: u32, tw: u32, th: u32) -> Vec<u8> {
    let mut out = vec![0u8; (tw * th * 4) as usize];
    if tw == 0 || th == 0 || sw == 0 || sh == 0 {
        return out;
    }
    for ty in 0..th {
        let (y0, y1) = (ty * sh / th, ((ty + 1) * sh / th).max(ty * sh / th + 1).min(sh));
        for tx in 0..tw {
            let (x0, x1) = (tx * sw / tw, ((tx + 1) * sw / tw).max(tx * sw / tw + 1).min(sw));
            let mut acc = [0u32; 4];
            let mut n = 0;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * sw + x) * 4) as usize;
                    let a = src[i + 3] as u32;
                    for c in 0..3 {
                        acc[c] += src[i + c] as u32 * a;
                    }
                    acc[3] += a;
                    n += 1;
                }
            }
            let o = ((ty * tw + tx) * 4) as usize;
            for c in 0..3 {
                out[o + c] = acc[c].checked_div(acc[3]).unwrap_or(0) as u8;
            }
            out[o + 3] = (acc[3] / n.max(1)) as u8;
        }
    }
    out
}

/// Upload an image under `id`, in chunks as the protocol requires.
fn transmit(out: &mut String, id: u32, r: &Raster) {
    let b64 = crate::base64(&r.rgba);
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(4096).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let more = (i + 1 < chunks.len()) as u8;
        let chunk = std::str::from_utf8(chunk).unwrap_or("");
        if i == 0 {
            out.push_str(&format!("\x1b_Ga=t,f=32,s={},v={},i={id},q=2,m={more};{chunk}\x1b\\", r.width, r.height));
        } else {
            out.push_str(&format!("\x1b_Gm={more};{chunk}\x1b\\"));
        }
    }
}

/// xterm's default colour for palette index `i`.
fn xterm_color(i: u8) -> [u8; 3] {
    const BASE: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 0, 0],
        [0, 205, 0],
        [205, 205, 0],
        [0, 0, 238],
        [205, 0, 205],
        [0, 205, 205],
        [229, 229, 229],
        [127, 127, 127],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [92, 92, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    match i {
        0..=15 => BASE[i as usize],
        16..=231 => {
            let i = i - 16;
            let v = |x: u8| if x == 0 { 0 } else { 55 + x * 40 };
            [v(i / 36), v(i / 6 % 6), v(i % 6)]
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            [g, g, g]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::view::Scale;
    use ratatui::style::Style;

    fn overlay(text: &str, scale: Scale, style: Style) -> TextOverlay {
        let chars: Vec<(char, u8)> = text.chars().map(|c| (c, 1)).collect();
        let width = (chars.len() as f32 * scale.factor()).ceil() as u16 + scale.s as u16;
        TextOverlay { x: 0, y: 3, scale, chars, style, width }
    }

    #[test]
    fn parses_probe_replies() {
        let s = "\x1b_Gi=31;OK\x1b\\\x1b]10;rgb:ffff/8080/0000\x1b\\\x1b[6;20;10t\x1b]4;1;rgb:cc/00/00\x07\x1b[?62;22c";
        let p = parse_probe(s);
        assert!(p.graphics);
        assert_eq!(p.fg, Some([255, 128, 0]));
        assert_eq!(p.cell, Some((10, 20)));
        assert_eq!(p.palette[&1], [0xcc, 0, 0]);
        assert!(!parse_probe("\x1b[?62c").graphics);
    }

    #[test]
    fn parses_ghostty_config() {
        let cfg = "# comment\nfont-family = \"Fira Code\"\nfont-family = Symbols Nerd Font\nfont-style-bold = default\n";
        let f = parse_ghostty_config(cfg, None, 0);
        assert_eq!(f.family.as_deref(), Some("Fira Code"));
        assert_eq!(f.style_bold, None);
        // An empty value resets the list.
        let f = parse_ghostty_config("font-family = A\nfont-family =\nfont-family = B\nfont-family-bold = C\n", None, 0);
        assert_eq!(f.family.as_deref(), Some("B"));
        assert_eq!(f.family_bold.as_deref(), Some("C"));
    }

    #[test]
    fn rasterizes_heading() {
        let mut h = HeadingImages::new(Face::new(embedded()).unwrap(), (10, 20));
        let o = overlay("Hello", Scale { s: 2, n: 0, d: 0 }, Style::default().fg(Color::Rgb(255, 0, 0)));
        let key = h.key(&o);
        let r = h.rasterize(&o, &key).unwrap();
        // 2x: five characters span ten columns, two rows.
        assert_eq!(h.block(&o), (10, 2));
        assert_eq!((r.width, r.height), (100, 40));
        let inked = r.rgba.chunks(4).filter(|p| p[3] > 128).count();
        assert!(inked > 200, "glyphs drawn: {inked}");
        assert!(r.rgba.chunks(4).filter(|p| p[3] > 128).all(|p| p[0] == 255 && p[1] == 0));
        // Fractional sizes need fewer columns.
        let o = overlay("Hello", Scale { s: 2, n: 3, d: 4 }, Style::default());
        assert_eq!(h.block(&o), (8, 2));
        // Uploads once, then only places.
        let first = h.draw(std::slice::from_ref(&o));
        assert!(first.contains("a=t,"));
        let again = h.draw(std::slice::from_ref(&o));
        assert!(!again.contains("a=t,") && again.contains("a=p,i=1,c=8,r=2"));
        // After a resize everything is uploaded and placed afresh.
        h.invalidate();
        let after = h.draw(std::slice::from_ref(&o));
        assert!(after.starts_with("\x1b_Ga=d,d=A") && after.contains("a=t,") && after.contains("a=p,i=2,"));
    }
}
