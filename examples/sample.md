---
title: mdvim sample
tags: [demo, markdown]
---

# Welcome to mdvim

A **vim-style** editor with *live preview*, like Obsidian. Move the cursor
onto a line to reveal its raw markdown; move away and it renders again.

## Inline styles

Text can be **bold**, *italic*, ***both***, ~~struck~~, ==highlighted==,
or `inline code`. Links look like [the mdvim repo](https://example.com/mdvim)
and autolinks like <https://example.com>. Footnotes work too[^1].

Escaped \*asterisks\* stay literal. Math: $e^{i\pi} + 1 = 0$.

![a diagram](images/diagram.png)

## Lists

- First item
- Second item with `code`
  - Nested item
    - Deeper still
- Back to the top

1. Ordered one
2. Ordered two
3. Ordered three

### Tasks

- [ ] Write the parser
- [x] Render headings
- [ ] Toggle me with `<leader>x`

## Quotes

> The best way to predict the future is to invent it.
> > Nested quotes get extra bars.

## Code

```rust
fn main() {
    println!("Hello, *not emphasis*");
}
```

## Tables

| Feature        | Status | Notes            |
|:---------------|:------:|-----------------:|
| Live preview   | ✅     | cursor line raw  |
| Vim motions    | ✅     | `w b e % f t`    |
| **Splits**     | ✅     | `:sp` / `:vs`    |

---

#### Heading four
##### Heading five
###### Heading six

<span>inline html</span> stays dimmed.

[^1]: This is the footnote text.
