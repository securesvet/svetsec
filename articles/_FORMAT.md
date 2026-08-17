# Article format

Create public articles as `articles/<slug>.md` on the `main` branch.

- Use a safe ASCII slug such as `rust-memory-safety.md`.
- The list title is derived from the filename: `rust-memory-safety` becomes
  `Rust memory safety`.
- Start the document with an H1 heading. Once the article is opened, this H1 is
  used as its full title.
- Standard headings, paragraphs, lists, blockquotes, and fenced code blocks are
  rendered by the browser and SSH interfaces.
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
