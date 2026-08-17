# svetsec.ru

One Ratatui interface for the terminal and the browser.

## Requirements

- Stable Rust with the `wasm32-unknown-unknown` target
- [Trunk](https://trunkrs.dev/) for the browser build

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
```

## Run in a terminal

```sh
cargo run -p svetsec-terminal
```

Use `Left`/`Right`, `h`/`l`, or `1`–`3` to switch tabs. Press `gx` to open
<https://svetsec.ru> in the default browser. Press `q` or `Esc` to exit.

## Run in a browser

```sh
trunk serve
```

Open <http://127.0.0.1:8080>. The browser version supports the same navigation keys and mouse clicks.

The layout switches to a compact two-row header on phone-sized screens. Touching a tab uses the same menu hit-testing as a mouse click.

## Deploy the static site

Build the optimized static bundle:

```sh
trunk build --release
```

The deployable site is now in `dist/`. One direct Cloudflare Pages deployment is:

```sh
npx wrangler pages deploy dist --project-name svetsec
```

After the first deployment, attach `svetsec.ru` as a custom domain in the Pages dashboard. A Git-connected deployment can use the same `trunk build --release` build step and `dist` output directory, provided the build image installs Rust, the WASM target, and Trunk first.

## Project layout

```text
crates/app-core      shared state and messages
crates/app-ui        shared Ratatui rendering and menu hit-testing
crates/app-terminal  native Crossterm event loop
crates/app-web       Ratzilla/WebAssembly adapter and HTML shell
```
