# mdvim

A vim-style terminal text editor with **Obsidian-like live markdown preview**.

Markdown files render in place: headings lose their `#`s and take on colour,
`**bold**` becomes bold, `- [ ]` becomes ☐, tables get box-drawing borders, and
so on. The line under the cursor — or the whole table / code block it sits in —
switches back to raw source so you can edit it, exactly like Obsidian's live
preview. Every source line stays one display line (soft-wrapped), so line
numbers, `j`/`k`, counts and ranges all behave like vim.

Everything else is modal vim editing: motions, operators, text objects,
registers, macros, marks, visual modes, ex commands, splits and tabs.
There is deliberately no plugin system.

## Build & run

```sh
cargo build --release
./target/release/mdvim notes.md          # or: cargo install --path .
mdvim -O a.md b.md                        # vertical splits (-o horizontal, -p tabs)
mdvim +42 file.md                         # start at line 42
mdvim -c 'set nu' file.md                 # run ex commands at startup
mdvim -u NONE file.md                     # skip the config file
mdvim --dump-render file.md --width 80    # print the rendered view (debugging)
```

## Live preview

| Markdown | Rendered as |
|---|---|
| `# Heading` … `######` | bold headings, larger where supported (see *Heading sizes*) |
| `**bold**` `*italic*` `~~strike~~` `==highlight==` | styled text, markers hidden |
| `` `code` `` | code-coloured text |
| `[text](url)`, `<https://…>` | underlined link text only |
| `![alt](img.png)` | `🖼 alt` |
| `- item` (nested) | muted `•` at every depth |
| `- [ ]` / `- [x]` | ☐ / ☑ (done items dimmed and struck through) |
| `> quote` | `▎` bar, italic text |
| fenced code | shaded block with a language label |
| tables | aligned columns with `│ ┼ ─` borders |
| `---` | full-width rule |
| footnotes `[^1]` | superscript `¹` |
| `#tag` | accent-coloured tag |
| front matter, inline HTML | dimmed |

* The cursor line (and every selected line in visual mode) is shown raw.
* A cursor anywhere in a **table** or **fenced code block** reveals the whole block.
* `:preview` toggles preview for the current window (`:preview on|off`);
  `:set nopreview` turns it off everywhere. The statusline shows
  `markdown ◆ preview` or `markdown ◇ source`.
* `:set ft=markdown` enables markdown handling for other file types.

### Heading sizes

Like Obsidian, headings get progressively smaller from `#` to `######`:

* In terminals that support the [kitty text-sizing protocol](https://sw.kovidgoyal.net/kitty/text-sizing-protocol/)
  (OSC 66), headings are drawn with a genuinely larger font: H1 2×, H2 1.75×,
  H3 1.5×, H4 1.25× (each using two rows); H5/H6 are normal size. kitty
  supports it.
* In terminals with the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/)
  but not text sizing — such as **Ghostty** and WezTerm — mdvim renders those
  headings itself, at the same sizes, and shows them as images. The font
  matches the terminal's: under Ghostty it uses your `font-family-bold` /
  `font-family` (and `font-style-bold`) settings, resolved with fontconfig, or
  Ghostty's built-in JetBrains Mono when none is set; elsewhere the system
  monospace font. Characters the font lacks (emoji, CJK) come from fontconfig
  fallbacks. Changing Ghostty's font size re-renders them.
* Elsewhere headings stay normal size and, like Obsidian's default theme, keep
  the text colour with no underline: H1/H2 bold and bright, H3/H4 bold, H5/H6
  bold and progressively muted.

Support is detected automatically at startup. `:set noheadingscale` turns real
sizing off; `MDVIM_NO_TEXT_SIZING=1` skips the text-sizing probe and
`MDVIM_NO_HEADING_IMAGES=1` the image fallback.

## Themes

Colour schemes work like Obsidian themes: each one is a small palette of
semantic colours — background, secondary background (code, menus), text,
muted and faint text, an accent, heading colours, and so on — that cascades
into every highlight group, and the scheme paints its own background.
Translucent touches such as tag pills and `==highlights==` are blended from the
palette the way Obsidian blends `rgba(accent, 0.15)`.

```
:colorscheme kanagawa      " switch (Tab completes; :colo for short)
:colorscheme               " show the current scheme
mdvim --colorscheme nord file.md
```

or set `colorscheme = "catppuccin-mocha"` at the top of `config.toml`.

| Family | Schemes |
|---|---|
| Terminal | `default` (uses your terminal's ANSI colours) |
| Obsidian | `obsidian`, `obsidian-light` |
| Kanagawa | `kanagawa` (wave), `kanagawa-dragon`, `kanagawa-lotus` |
| Tokyo Night | `tokyonight`, `tokyonight-storm`, `tokyonight-moon`, `tokyonight-day` |
| Catppuccin | `catppuccin-mocha` (alias `catppuccin`), `catppuccin-macchiato`, `catppuccin-frappe`, `catppuccin-latte` |
| Rosé Pine | `rose-pine`, `rose-pine-moon`, `rose-pine-dawn` |
| Others | `gruvbox`, `gruvbox-light`, `nord`, `dracula`, `everforest` |

Schemes other than `default` use 24-bit colour, so your terminal needs
truecolor support (almost all modern terminals have it).

### Writing your own

Drop a TOML file in `~/.config/mdvim/themes/NAME.toml` and load it with
`:colorscheme NAME` (a path to any `.toml` file works too). A file with the
same name as a built-in replaces it. The built-in schemes in
[`themes/`](themes/) are written in the same format, so they make good
starting points.

```toml
name = "My Theme"
base = "tokyonight"        # optional: inherit everything, override below

[palette]
bg = "#1b1d2b"             # bg, fg and accent are required without a base
fg = "#c8d3f5"
accent = "#ff966c"
h1 = "#ff757f"

[groups]                   # optional per-group tweaks; colours may name palette keys
h1 = { fg = "h1", bold = true, underline = true }
quote = { fg = "fg_muted", italic = true }
```

Palette keys (anything left out falls back to a sensible neighbour):

| Key | Used for | Falls back to |
|---|---|---|
| `bg`, `fg`, `accent` | background, text, links/tags/checkboxes/quote bars/mode badge | — |
| `bg_dark` | inactive statusline, tabline | `bg` |
| `bg_alt` | inline code, code blocks, completion menu | `bg_dark` |
| `bg_hl`, `selection`, `statusline` | cursorline, visual selection, statusline | `bg_alt` / `bg_hl` / `bg_alt` |
| `fg_muted`, `fg_faint` | struck text, quotes; line numbers, markup, front matter | `fg` / `fg_muted` |
| `border` | splits, table borders, rules | `fg_faint` |
| `red` `orange` `yellow` `green` `cyan` `blue` `purple` | modes, search, messages | terminal colours |
| `h1` … `h6` | headings | `fg` |
| `bold`, `italic`, `code`, `quote` | inline styles | `fg` |
| `link`, `tag`, `tag_bg` | links, `#tags` and their pill | `accent` / `accent` / tag blended into bg |
| `listmarker`, `highlight_bg`, `math`, `cursorlinenr` | bullets, `==text==`, math, current line number | `fg_faint` / yellow blended into bg / `cyan` / `accent` |

Groups set under `[theme]` in `config.toml` are applied on top of whichever
scheme is active, and survive `:colorscheme` switches; there, too, colours may
name palette keys (`link = { fg = "cyan", underline = true }` uses the scheme's
own cyan).

### Markdown editing helpers

* `Enter` / `o` continue lists, numbered lists (auto-incremented and renumbered),
  task lists and block quotes. `Enter` on an empty item ends the list.
* `Tab` / `Shift-Tab` in insert mode indent / outdent a list item.
* `<leader>x` (default leader: space) toggles a task checkbox — on a plain line
  it creates a task. Also available as `:task` (works on ranges).
* Headings fold their sections: `za` `zc` `zo` `zR` `zM`.
* `]]` / `[[` jump between headings.
* `gf` opens the linked file under the cursor (`[x](other.md)`), `gx` opens a URL.
* `gq{motion}` reflows to `textwidth`, keeping list indentation.

## Vim features

**Modes**: normal, insert, replace (`R`), visual (`v`, `V`, `Ctrl-v`), command line
(`:`, `/`, `?`), plus `Ctrl-o` from insert.

**Motions**: `h j k l` `w b e ge W B E gE` `0 ^ $ g_ |` `gg G N% ` `f F t T ; ,`
`% { } ( ) ]] [[` `H M L` `n N * #` `` `x 'x `` `+ - _ Enter` `gj gk`, all with counts.

**Operators**: `d c y > < g~ gu gU g? gq gw`, doubled for lines (`dd`, `gUU`, …),
combined with any motion or text object, plus `x X D C s S Y r J gJ ~ p P gp gP`
and `Ctrl-a` / `Ctrl-x` (decimal and hex).

**Text objects**: `iw aw iW aW is as ip ap i( a( ib i[ i{ iB i< i" i' i` it at`.

**Visual mode**: all operators, `o`/`O`, `I`/`A` block insert, `r`, `J`, `p`
(replaces the selection), `u U ~`, `>`/`<` with counts, `g Ctrl-a` sequences, `gv`.

**Registers**: `"a`–`"z` (uppercase appends), `"0`–`"9`, `"-`, `"_`, `".`, `":`, `"/`,
and `"+`/`"*` for the system clipboard (via `wl-copy`/`xclip`/`xsel`/`pbcopy`,
falling back to OSC 52 for copying).

**Repeat & macros**: `.` (with counts), `q{reg}` … `q`, `@{reg}`, `@@`, `@:`.

**Undo**: `u`, `Ctrl-r`; each command (including a whole insert session or `:g`) is one step.

**Marks & jumps**: `m{a-zA-Z}`, `'` / `` ` ``, `''`, `` `. `` `'^` `'[` `']` `'<` `'>`,
`Ctrl-o` / `Ctrl-i`.

**Search**: `/` `?` with incremental preview and highlighting, `n N * #`, `:noh`.
Ignorecase + smartcase by default.

**Insert mode**: `Ctrl-w Ctrl-u Ctrl-h Ctrl-t Ctrl-d Ctrl-r{reg} Ctrl-o Ctrl-v Ctrl-e Ctrl-y Ctrl-a`,
`Ctrl-n`/`Ctrl-p` word completion (from all buffers) with a popup menu, autoindent,
counted inserts (`3ihi<Esc>`).

**Windows**: `Ctrl-w s v n w W p h j k l t b q c o = + - < > _ | x T`;
`:sp`, `:vs`, `:new`, `:vnew`, `:close`, `:only`, `:resize`, `:vertical resize`.

**Tabs**: `gt`, `gT`, `{N}gt`, `:tabnew`, `:tabe`, `:tabn`, `:tabp`, `:tabc`, `:tabo`,
`:tabfirst`, `:tablast`; click the tabline to switch.

**Buffers**: `:e`, `:e!`, `:enew`, `:ls`, `:b N|name`, `:bn`, `:bp`, `:bd[!]`, `Ctrl-^`.

**Ex commands** (with ranges `N . $ % 'a '<,'> /pat/ ?pat? +N -N`, `,` and `;`):
`:w[!] [file]`, `:{range}w file`, `:wq`, `:x`, `:q[!]`, `:qa[!]`, `:wa`, `:wqa`, `:up`,
`:sav`, `:r file`, `:r !cmd`, `:!cmd`, `:{range}!filter`,
`:s/pat/rep/[gieIn]` (with `\1`, `&`, `\u \U \l \L \e`, `\r`), `:&`, `:&&`,
`:g/pat/cmd`, `:v/pat/cmd`, `:d`, `:y`, `:pu`, `:m`, `:t`/`:co`, `:j`, `:>`, `:<`,
`:norm[!]`, `:sort [n][i][u][!]`, `:retab`, `:set` (`opt`, `noopt`, `opt!`, `invopt`,
`opt?`, `opt=N`, `opt+=N`, `opt-=N`), `:setlocal preview`,
`:map`/`:noremap` families (`n v x i o`) and `:unmap`, `:reg`, `:marks`, `:jumps`,
`:{N}`, `:=`, `:mark`, `:cd`, `:pwd`, `:help`.
The command line has history (`Up`/`Down`, prefix-filtered), `Tab` completion for
commands, file paths, options and buffers, and `Ctrl-r {reg}` / `Ctrl-r Ctrl-w`.

**Mouse**: click to move/focus, drag to select, wheel to scroll. Bracketed paste.
`Ctrl-z` suspends.

## Configuration

`~/.config/mdvim/config.toml` — see [`examples/config.toml`](examples/config.toml)
for every setting. It covers options, non-recursive key mappings per mode
(`[keymaps.normal]`, `insert`, `visual`, `operator`), the leader key, and colours
the colour scheme (`colorscheme = "…"`, see [Themes](#themes)), and overrides
for every highlight group (UI groups like `statusline`, `visual`, `search`, and
markdown groups `h1`…`h6`, `bold`, `code`, `codeblock`, `link`, `quote`,
`listmarker`, `taskbox`, `taskdone`, `tableborder`, `markup`, …).

## Differences from Neovim

* No plugins, Lua, LSP, tree-sitter, `:terminal`, spell checking or swap files.
* Search and `:s` patterns use **Rust regex syntax** (`(a|b)+`, `\d`, `\b`), not
  vim's magic syntax; `\<` `\>` `\c` `\C` are translated.
* Undo is linear (no undo tree / `:earlier`).
* `:s` with the `c` (confirm) flag substitutes without prompting.
* Block-visual columns are character columns (tabs and wide characters are not
  measured in screen cells).
* Only heading-based folds (markdown); no `foldmethod`.
* Syntax highlighting is markdown-only; other files are shown plain.

## Development

```sh
cargo test                       # headless key-feed tests for editing + render tests
cargo clippy -- -D warnings
```

The core (`src/editor`) is a terminal-independent state machine driven by
`Editor::handle_key`; `src/ui` draws it with ratatui, and `src/markdown` turns
the buffer into per-byte style/conceal annotations using pulldown-cmark.
