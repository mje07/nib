# Writing a colour scheme

A scheme is a TOML file. Put yours in `~/.config/nib/themes/NAME.toml` and
load it with `:colorscheme NAME` (a path to any `.toml` file also works). A
file with the same name as a built-in replaces it. Every file in this
directory is a built-in written in the same format, so copy one to start.

```toml
name = "My Theme"
base = "tokyonight"        # optional: inherit everything, override below

[palette]
bg = "#1b1d2b"             # bg, fg and accent are required without a base
fg = "#c8d3f5"
accent = "#ff966c"
h1 = "#ff757f"

[groups]                   # optional per-group tweaks
h1 = { fg = "h1", bold = true, underline = true }
quote = { fg = "fg_muted", italic = true }
```

Like an Obsidian theme, the palette is a handful of semantic colours that
cascade into every highlight group. Anything you leave out falls back to a
sensible neighbour.

| Key | Used for | Falls back to |
|---|---|---|
| `bg`, `fg`, `accent` | background, text; links, tags, checkboxes, quote bars | required |
| `bg_dark` | inactive statusline, tabline | `bg` |
| `bg_alt` | inline code, code blocks, completion menu | `bg_dark` |
| `bg_hl` | cursorline | `bg_alt` |
| `selection` | visual selection | `bg_hl` |
| `statusline` | active statusline | `bg_alt` |
| `fg_muted` | struck text, quotes | `fg` |
| `fg_faint` | line numbers, markup, front matter | `fg_muted` |
| `border` | splits, table borders, rules | `fg_faint` |
| `red` `orange` `yellow` `green` `cyan` `blue` `purple` | mode badges, search, messages | terminal colours |
| `h1` ... `h6` | headings | `fg` |
| `bold`, `italic`, `code`, `quote` | inline styles | `fg` |
| `link`, `tag` | links, `#tags` | `accent` |
| `tag_bg` | the pill behind a tag | `tag` blended into `bg` |
| `highlight_bg` | `==highlighted==` text | `yellow` blended into `bg` |
| `listmarker` | bullets and numbers | `fg_faint` |
| `math` | `$math$` | `cyan` |
| `cursorlinenr` | current line number | `accent` |

Colours are hex (`"#ff79c6"`), terminal names (`"blue"`, `"lightmagenta"`) or
0-255 palette indices. In `[groups]`, and in the `[theme]` table of
`config.toml`, a colour may also name a palette key, so `{ fg = "red" }` means
the scheme's own red. Group styles also take `bold`, `italic`, `underline`,
`dim`, `reversed` and `strikethrough`.

Highlight groups: `normal`, `linenr`, `cursorlinenr`, `cursorline`, `nontext`,
`visual`, `search`, `incsearch`, `statusline`, `statuslinenc`, `mode_normal`,
`mode_insert`, `mode_visual`, `mode_replace`, `mode_command`, `tabline`,
`tablinesel`, `tablinefill`, `vertsplit`, `error`, `warning`, `moremsg`,
`fold`, `pmenu`, `pmenusel`, `matchparen`, `h1`-`h6`, `bold`, `italic`,
`strike`, `highlight`, `code`, `codeblock`, `codefence`, `link`, `url`,
`image`, `quote`, `quotebar`, `listmarker`, `taskbox`, `taskdone`, `tag`,
`hr`, `tableborder`, `tableheader`, `markup`, `html`, `math`, `footnote`,
`frontmatter`.
