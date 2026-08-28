# Article format

Create one directory per public article on the `main` branch:

```text
articles/
  rust-memory-safety/
    en.md
    ru.md
```

An article may contain only one language file. The reader first asks for the
visitor's selected language and falls back to the other file when that
translation does not exist.

Optional frontmatter can assign up to six labels:

```yaml
---
labels:
  - cryptography
  - python
---
```

Label matching is case-insensitive. Known labels use curated colors; every
other label is mapped deterministically to the same palette color.

- Use a safe ASCII directory slug such as `rust-memory-safety`.
- The list title is derived from the directory: `rust-memory-safety` becomes
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
