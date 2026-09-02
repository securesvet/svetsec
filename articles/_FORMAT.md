# Article format

Create one directory per public article on the `main` branch:

```text
articles/
  rust-memory-safety/
    en.md
    ru.md
```

Every public article must contain both `en.md` and `ru.md`. The reader asks for
the visitor's selected language first; the fallback remains in place so an
article still opens safely while a translation is being prepared.

Frontmatter is preloaded for the article index. Give both language variants the
same ISO date and a localized title; labels remain optional (up to six):

```yaml
---
title: "Memory safety in Rust"
date: 2026-09-02
labels:
  - cryptography
  - python
---
```

Label matching is case-insensitive. Known labels use curated colors; every
other label is mapped deterministically to the same palette color.

- Use a safe ASCII directory slug such as `rust-memory-safety`.
- Articles are sorted newest-first and separated into date groups. Use
  `YYYY-MM-DD`; an absent or invalid value is grouped under `Undated`.
- The list uses the localized frontmatter `title`. If it is missing, the title
  is derived from the directory: `rust-memory-safety` becomes
  `Rust memory safety`.
- Start the document with an H1 heading. Once the article is opened, this H1 is
  used as its full title.
- Standard headings, paragraphs, lists, blockquotes, and fenced code blocks are
  rendered by the browser and SSH interfaces.
- Articles open in View mode. Press `i` to enable read-only Vim mode, then use
  `h`/`j`/`k`/`l` (or arrows) to move
  by character and line, and `0`/`$` for line boundaries. Fenced blocks render
  as one bordered code panel with syntax highlighting and a Copy action.
  Fenced `python`, `python3`, or `py` blocks also expose Run and can be executed
  with `p`; the server runs only the selected committed block through Pyodide
  with time and output limits.
- Use an empty fenced `dot-well` block to embed the shared animated Gaussian
  dot field. It renders inside the article in both the browser and SSH.
- Put article images under `articles/assets/` and reference them on a separate
  line, for example `![Pixel Earth](assets/earth.png)`. PNG, JPEG, and WebP are
  displayed as regular responsive images in the browser and as true-color
  terminal previews over SSH.
- Files whose names start with `_` are documentation and are not listed as
  articles.

Example:

````markdown
# Memory safety in Rust

Short introduction.

## A section

- First point
- Second point

> A useful quotation.

```rust
fn main() {
    println!("hello");
}
```
````
