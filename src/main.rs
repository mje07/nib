mod buffer;
mod config;
mod editor;
mod headings;
mod key;
mod markdown;
mod options;
mod theme;
mod types;
mod ui;

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, MouseButton,
    MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::{Terminal, buffer::Buffer as TBuf, layout::Rect as TRect};

use editor::Editor;
use editor::layout::Dir;
use key::Key;
use ui::CursorShape;

/// A vim-style terminal editor with Obsidian-like live markdown preview.
#[derive(Parser, Debug)]
#[command(name = "mdvim", version, about)]
struct Cli {
    /// Files to edit
    files: Vec<PathBuf>,
    /// Open files in horizontal splits
    #[arg(short = 'o')]
    hsplit: bool,
    /// Open files in vertical splits
    #[arg(short = 'O')]
    vsplit: bool,
    /// Open files in tabs
    #[arg(short = 'p')]
    tabs: bool,
    /// Config file to use ("NONE" to skip loading any config)
    #[arg(short = 'u', value_name = "CONFIG")]
    config: Option<String>,
    /// Ex command to run after loading the first file (like vim's -c)
    #[arg(short = 'c', value_name = "CMD")]
    commands: Vec<String>,
    /// Debug: print FILE rendered as the editor would show it, then exit
    #[arg(long, value_name = "FILE")]
    dump_render: Option<PathBuf>,
    /// Width for --dump-render
    #[arg(long, default_value_t = 80)]
    width: u16,
    /// Cursor line (1-based) for --dump-render; that line is shown raw
    #[arg(long)]
    cursor: Option<usize>,
    /// Emit ANSI colours with --dump-render
    #[arg(long)]
    ansi: bool,
    /// Colour scheme to use (overrides the config file; see :colorscheme)
    #[arg(long, value_name = "NAME")]
    colorscheme: Option<String>,
}

fn main() {
    // Vim-style `+N` / `+` (go to line) arguments are handled before clap.
    let mut goto: Option<Option<usize>> = None;
    let args: Vec<String> = std::env::args()
        .filter(|a| {
            if let Some(n) = a.strip_prefix('+') {
                goto = Some(n.parse().ok());
                false
            } else {
                true
            }
        })
        .collect();
    let cli = Cli::parse_from(args);
    if let Some(path) = &cli.dump_render {
        if let Err(e) = dump_render(path, cli.width, cli.cursor, cli.ansi, cli.colorscheme.as_deref()) {
            eprintln!("mdvim: {e}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(e) = run(cli, goto) {
        eprintln!("mdvim: {e}");
        std::process::exit(1);
    }
}

fn setup_editor(cli: &Cli, w: u16, h: u16, goto: Option<Option<usize>>) -> Editor {
    let mut ed = Editor::new(w, h);
    let mut errs = Vec::new();
    match cli.config.as_deref() {
        Some("NONE") => {}
        Some(p) => errs = config::load(&mut ed, std::path::Path::new(p)),
        None => {
            if let Some(p) = config::default_path() {
                errs = config::load(&mut ed, &p);
            }
        }
    }
    if let Some(name) = &cli.colorscheme
        && let Err(e) = ed.set_colorscheme(name)
    {
        errs.push(e);
    }
    ed.registers.use_clipboard = ed.options.clipboard;
    if !cli.files.is_empty() {
        ed.open_initial(&cli.files[..1]);
        for f in &cli.files[1..] {
            let id = match ed.buffer_for_path(f) {
                Ok(id) => id,
                Err(e) => {
                    ed.error(e);
                    continue;
                }
            };
            if cli.hsplit || cli.vsplit {
                let dir = if cli.vsplit { Dir::Vertical } else { Dir::Horizontal };
                let saved = (ed.options.splitbelow, ed.options.splitright);
                ed.options.splitbelow = true;
                ed.options.splitright = true;
                ed.split(dir, Some(id));
                (ed.options.splitbelow, ed.options.splitright) = saved;
            } else if cli.tabs {
                ed.new_tab(id);
            }
        }
        if cli.hsplit || cli.vsplit {
            editor::layout::equalize(&mut ed.tab_mut().root);
            let first = editor::layout::leaves(&ed.tab().root)[0];
            ed.focus_window(first);
        } else if cli.tabs {
            ed.goto_tab(0);
        }
        if cli.files.len() > 1 && !(cli.hsplit || cli.vsplit || cli.tabs) {
            ed.info(format!("{} files loaded; :ls lists them, :bn switches", cli.files.len()));
        }
    }
    if let Some(g) = goto {
        let last = ed.buf().line_count();
        let l = g.unwrap_or(last).clamp(1, last) - 1;
        let c = ed.buf().first_non_blank(l);
        ed.set_cursor_col(types::Pos::new(l, c));
    }
    for c in &cli.commands {
        if let Err(e) = ed.run_ex(c) {
            ed.error(e);
        }
        ed.commit();
    }
    if !errs.is_empty() {
        ed.show_lines(errs);
    }
    ed.scroll_cursor_to(editor::view::ScrollPos::Center);
    ed.scroll_to_cursor();
    ed
}

fn restore_terminal() {
    let mut out = io::stdout();
    let _ = execute!(out, DisableMouseCapture, DisableBracketedPaste, LeaveAlternateScreen, SetCursorStyle::DefaultUserShape);
    let _ = terminal::disable_raw_mode();
}

fn init_terminal(mouse: bool) -> io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableBracketedPaste)?;
    if mouse {
        execute!(out, EnableMouseCapture)?;
    }
    Ok(())
}

fn run(cli: Cli, goto: Option<Option<usize>>) -> anyhow::Result<()> {
    let (w, h) = terminal::size().unwrap_or((80, 24));
    let mut ed = setup_editor(&cli, w, h, goto);

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        hook(info);
    }));
    init_terminal(ed.options.mouse)?;
    // The alternate screen starts blank, so no initial clear is needed
    // (ratatui's clear() queries the cursor position, which can stall).
    ed.text_sizing = detect_text_sizing();
    // No text sizing: draw scaled headings as images where graphics work.
    let mut images = if ed.text_sizing { None } else { headings::HeadingImages::detect() };
    if images.is_some() {
        ed.text_sizing = true;
    }
    ed.scroll_to_cursor();
    let mut term = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut last_overlays: Vec<ui::TextOverlay> = Vec::new();
    let mut shape = None;
    let mut mouse_on = ed.options.mouse;

    loop {
        // Synchronized output: the terminal shows the frame only once complete.
        write!(io::stdout(), "\x1b[?2026h")?;
        let (cursor_shape, overlays, cursor_pos) = draw_frame(&mut term, &ed)?;
        if let Some(img) = &mut images {
            // Images sit above the cells, so ratatui's output is unaffected:
            // just swap the placements.
            if overlays != last_overlays {
                let mut out = io::stdout();
                out.write_all(img.draw(&overlays).as_bytes())?;
                if let Some((x, y)) = cursor_pos {
                    execute!(out, ratatui::crossterm::cursor::MoveTo(x, y))?;
                }
                last_overlays = overlays;
            }
        } else if overlays != last_overlays {
            // Scaled text spans several cells that ratatui does not know about;
            // clear the screen and repaint everything before drawing the new set.
            if !last_overlays.is_empty() {
                execute!(io::stdout(), terminal::Clear(terminal::ClearType::All))?;
                term = Terminal::new(CrosstermBackend::new(io::stdout()))?;
                draw_frame(&mut term, &ed)?;
            }
            let mut out = io::stdout();
            for o in &overlays {
                out.write_all(o.escape().as_bytes())?;
            }
            if let Some((x, y)) = cursor_pos {
                execute!(out, ratatui::crossterm::cursor::MoveTo(x, y))?;
            }
            last_overlays = overlays;
        }
        write!(io::stdout(), "\x1b[?2026l")?;
        io::stdout().flush()?;
        if shape != Some(cursor_shape) {
            let style = match cursor_shape {
                CursorShape::Block => SetCursorStyle::SteadyBlock,
                CursorShape::Bar => SetCursorStyle::SteadyBar,
                CursorShape::Underline => SetCursorStyle::SteadyUnderScore,
            };
            execute!(io::stdout(), style)?;
            shape = Some(cursor_shape);
        }
        if let Some(text) = ed.registers.clipboard.osc52.borrow_mut().take() {
            let b64 = base64(text.as_bytes());
            let mut out = io::stdout();
            write!(out, "\x1b]52;c;{b64}\x07")?;
            out.flush()?;
        }
        if ed.options.mouse != mouse_on {
            mouse_on = ed.options.mouse;
            if mouse_on {
                execute!(io::stdout(), EnableMouseCapture)?;
            } else {
                execute!(io::stdout(), DisableMouseCapture)?;
            }
        }
        if ed.quit {
            break;
        }
        if ed.suspend {
            ed.suspend = false;
            if images.is_some() {
                write!(io::stdout(), "{}", headings::HeadingImages::clear())?;
            }
            restore_terminal();
            unsafe {
                libc::raise(libc::SIGTSTP);
            }
            init_terminal(ed.options.mouse)?;
            execute!(io::stdout(), terminal::Clear(terminal::ClearType::All))?;
            // A fresh Terminal has empty buffers, forcing a full redraw.
            term = Terminal::new(CrosstermBackend::new(io::stdout()))?;
            last_overlays.clear();
            shape = None;
            continue;
        }
        let timeout = if ed.has_pending_map() { Duration::from_millis(ed.options.timeoutlen) } else { Duration::from_secs(3600) };
        if !event::poll(timeout)? {
            ed.on_timeout();
            continue;
        }
        loop {
            match event::read()? {
                Event::Key(k) => {
                    if let Some(key) = Key::from_event(k) {
                        // Like terminal vim, Alt+key is Esc followed by key (this
                        // also covers a fast "<Esc>:" arriving as one sequence).
                        if key.alt && !key.ctrl && matches!(key.code, key::KeyCode::Char(_)) {
                            ed.handle_key(Key::ESC);
                            ed.handle_key(Key { alt: false, ..key });
                        } else {
                            ed.handle_key(key);
                        }
                    }
                }
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => ed.mouse_down(m.column, m.row),
                    MouseEventKind::Drag(MouseButton::Left) => ed.mouse_drag_to(m.column, m.row),
                    MouseEventKind::Up(_) => ed.mouse_up(),
                    MouseEventKind::ScrollDown => ed.mouse_scroll(m.column, m.row, true),
                    MouseEventKind::ScrollUp => ed.mouse_scroll(m.column, m.row, false),
                    _ => {}
                },
                Event::Paste(s) => ed.paste(&s),
                Event::Resize(w, h) => {
                    ed.resize(w, h);
                    if let Some(img) = &mut images {
                        img.invalidate();
                        // Redraw the headings even if their positions are unchanged.
                        last_overlays.clear();
                    }
                }
                _ => {}
            }
            if ed.quit || !event::poll(Duration::from_millis(0))? {
                break;
            }
        }
    }
    drop(term);
    if images.is_some() {
        write!(io::stdout(), "{}", headings::HeadingImages::clear())?;
    }
    restore_terminal();
    Ok(())
}

type Frame = (CursorShape, Vec<ui::TextOverlay>, Option<(u16, u16)>);

fn draw_frame(term: &mut Terminal<CrosstermBackend<io::Stdout>>, ed: &Editor) -> io::Result<Frame> {
    let mut result = (CursorShape::Block, Vec::new(), None);
    term.draw(|f| {
        let area = f.area();
        let res = ui::draw(ed, f.buffer_mut(), area);
        if let Some((x, y)) = res.cursor {
            f.set_cursor_position((x, y));
        }
        result = (res.shape, res.overlays, res.cursor);
    })?;
    Ok(result)
}

/// Probe for the kitty text-sizing protocol (OSC 66): print a scaled space
/// and check, via cursor position reports, that the cursor moved two cells.
fn detect_text_sizing() -> bool {
    use ratatui::crossterm::cursor::{MoveTo, position};
    let mut out = io::stdout();
    if std::env::var_os("MDVIM_NO_TEXT_SIZING").is_some() {
        return false;
    }
    let mut probe = || -> io::Result<bool> {
        execute!(out, MoveTo(0, 0))?;
        let (x0, _) = position()?;
        write!(out, "\x1b]66;s=2; \x07")?;
        out.flush()?;
        let (x1, _) = position()?;
        Ok(x1 == x0 + 2)
    };
    let ok = probe().unwrap_or(false);
    let _ = execute!(io::stdout(), terminal::Clear(terminal::ClearType::All), MoveTo(0, 0));
    ok
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Render a file as the editor would display it and print it to stdout.
fn dump_render(path: &PathBuf, width: u16, cursor: Option<usize>, ansi: bool, colorscheme: Option<&str>) -> anyhow::Result<()> {
    let mut ed = Editor::new(width, 10);
    if let Some(name) = colorscheme {
        ed.set_colorscheme(name).map_err(|e| anyhow::anyhow!(e))?;
    }
    ed.open_initial(std::slice::from_ref(path));
    ed.messages.clear();
    // Tall enough to show the whole document.
    let rows: usize = (0..ed.buf().line_count())
        .map(|l| {
            let text = ed.buf().line(l);
            unicode_width::UnicodeWidthStr::width(text.as_str()) / width.max(1) as usize + 1
        })
        .sum();
    let height = (rows + 2).min(u16::MAX as usize / 2) as u16;
    ed.resize(width, height);
    match cursor {
        Some(l) => {
            let l = l.clamp(1, ed.buf().line_count()) - 1;
            ed.set_cursor(types::Pos::new(l, 0));
        }
        // Park the cursor past the end so every line is rendered.
        None => ed.win_mut().cursor = types::Pos::new(usize::MAX / 2, 0),
    }
    ed.win_mut().top = 0;
    let area = TRect::new(0, 0, width, height);
    let mut buf = TBuf::empty(area);
    ui::draw(&ed, &mut buf, area);
    let mut out = io::stdout().lock();
    // Rows of the window text area (skip statusline and command line).
    let text_rows = ed.text_rect(ed.win_id()).h;
    let mut last_nonempty = 0;
    let mut lines = Vec::new();
    for y in 0..text_rows {
        let mut line = String::new();
        let mut prev_style = None;
        let mut skip = 0;
        for x in 0..width {
            let c = &buf[(x, y)];
            if skip > 0 {
                skip -= 1;
                continue;
            }
            skip = unicode_width::UnicodeWidthStr::width(c.symbol()).saturating_sub(1);
            if ansi {
                let st = (c.fg, c.bg, c.modifier);
                if prev_style != Some(st) {
                    line.push_str(&sgr(c));
                    prev_style = Some(st);
                }
            }
            line.push_str(c.symbol());
        }
        if ansi {
            line.push_str("\x1b[0m");
        }
        let plain = line.trim_end().to_string();
        if !plain.starts_with('~') {
            last_nonempty = lines.len() + 1;
        }
        lines.push(if ansi { line } else { plain });
    }
    for l in &lines[..last_nonempty] {
        writeln!(out, "{l}")?;
    }
    Ok(())
}

fn sgr(c: &ratatui::buffer::Cell) -> String {
    ui::sgr(Some(c.fg), Some(c.bg), c.modifier)
}
