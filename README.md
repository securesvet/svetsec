# svetsec.ru

One Ratatui interface for the browser, a local terminal, and SSH. The server
adds SQLite-backed owner sessions, live owner presence, and articles.

## What is implemented

- The dot beside `svetsec.ru` is green while an authenticated owner session is
  active in the browser or over SSH. Presence expires after 45 seconds without
  a heartbeat.
- `r` switches the language in the browser and terminal; the destination
  country's flag appears for 1.5 seconds as confirmation.
- Desktop mouse hover shows a short single-line hint in the footer. Hovering
  the logo explains the green presence dot.
- Browser owner login uses an Argon2id password hash and an opaque
  `HttpOnly; SameSite=Strict` cookie. Only a SHA-256 hash of the random session
  token is stored in SQLite.
- SSH identifies the owner by username plus a verified public key. Other
  usernames get a read-only guest session.
- The owner can open Articles and press `e` over SSH to enter the Vim-like
  editor. Article writes are enforced server-side.
- Published Markdown is discovered from `main/articles` through the server.
  The filename list loads first; a file body is fetched only when opened.

## Requirements

- Stable Rust with the `wasm32-unknown-unknown` target
- [Trunk](https://trunkrs.dev/) for the browser build
- `ssh-keygen` for the SSH host key

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
```

## Build the browser

```sh
trunk build --release
```

The server serves the resulting `dist/` directory itself. A static-only Pages
deployment is no longer sufficient because sessions, presence, articles, and
SSH require the Rust server and its SQLite file.

## Configure and run the server

Generate a password hash (do not store the clear-text password in an env file):

```sh
cargo run -p svetsec-server -- hash-password 'choose-a-long-password'
```

Generate a persistent SSH host key once:

```sh
ssh-keygen -t ed25519 -N '' -f ./ssh_host_ed25519_key
```

Then set the configuration and run:

```sh
export SVETSEC_OWNER_PASSWORD_HASH='<argon2id output>'
export SVETSEC_OWNER_PUBLIC_KEY_FILE='/absolute/path/to/your/id_ed25519.pub'
export SVETSEC_SSH_HOST_KEY_FILE='/absolute/path/to/ssh_host_ed25519_key'
export SVETSEC_DATABASE='/var/lib/svetsec/svetsec.db'
export SVETSEC_HTTP_ADDR='127.0.0.1:3000'
export SVETSEC_SSH_ADDR='0.0.0.0:2222'
export SVETSEC_OWNER_USER='owner'
export SVETSEC_STATIC_DIR='dist'
export SVETSEC_GITHUB_REPOSITORY='securesvet/svetsec'
export SVETSEC_GITHUB_BRANCH='main'
export SVETSEC_SECURE_COOKIE='true'
cargo run --release -p svetsec-server
```

Terminate TLS in a reverse proxy and forward HTTPS to `127.0.0.1:3000`. Expose
the configured SSH port directly. Keep the database and SSH host key on
persistent storage with permissions limited to the service user.

For local HTTP development only, set `SVETSEC_SECURE_COOKIE=false` so the cookie
can be sent over `http://127.0.0.1`.

## Use the site

Browser shortcuts:

- `Left`/`Right`, `h`/`l`, or `1`–`3`: sections
- `r`: switch language
- `a`: owner password login
- `j`/`k` or arrows: select an article
- `Enter`/`o`: open the selected Markdown article
- `e`: edit the selected article on GitHub; creates one if the list is empty
- `n`: create a new Markdown article on GitHub
- `f`: refresh the filename list after a GitHub commit
- Mouse/touch: sections
- Mouse hover on desktop: contextual single-line hints

SSH guest:

```sh
ssh -p 2222 guest@svetsec.ru
```

SSH owner (the key must match `SVETSEC_OWNER_PUBLIC_KEY_FILE`):

```sh
ssh -p 2222 -i ~/.ssh/id_ed25519 owner@svetsec.ru
```

In the owner session, open Articles (`2`) and press `e`. Editor commands:

- `i`, `a`, `o`: enter Insert mode
- `Esc`: return to Normal mode
- `h`, `j`, `k`, `l`, `x`: Vim-style movement/deletion
- `:title TEXT`, `:slug SLUG`: edit metadata
- `:lang en` / `:lang ru`: switch the article language buffer
- `:publish` / `:draft`: change publication state
- `:export`: write the current language buffer to `articles/<slug>.md` so it
  can be committed and pushed to `main`
- `:w`, `:wq`, `:q!`: save, save-and-close, discard-and-close

Normal-mode and site shortcuts also accept the corresponding Russian-layout
keys, for example `h/р`, `j/о`, `k/л`, `l/д`, `r/к`, `e/у`, and `q/й`.

## Markdown articles from GitHub

Push public articles to `articles/<slug>.md` on `main`. The list title is
derived from the filename, while the full file and its `# H1` title are loaded
only when a visitor opens the article. Files beginning with `_` are ignored;
see `articles/_FORMAT.md` for the supported skeleton.

Public repositories need no GitHub token. If the repository becomes private or
the anonymous API limit is too small, set `SVETSEC_GITHUB_TOKEN` on the server
to a fine-grained token with read-only Contents permission. Never expose it to
the WASM client.

## API and data model

The server creates the SQLite schema automatically. It contains
`web_sessions`, `presence`, and `articles` tables and enables WAL mode.

- `GET/POST/DELETE /api/session`: state, login, logout
- `POST /api/heartbeat`: refresh owner presence
- `GET /api/articles`: published articles for guests, drafts included for owner
- `POST /api/articles`: create/update by slug; owner session required
- `GET /api/github/articles`: cached Markdown filename list from GitHub
- `GET /api/github/articles/:slug`: lazily fetched Markdown body

## Local terminal

```sh
cargo run -p svetsec-terminal
```

The local binary renders the shared UI but is intentionally not treated as an
authenticated owner session. Use the SSH endpoint for key-backed owner access.

## Project layout

```text
crates/app-core      shared state, localization, and messages
crates/app-ui        shared Ratatui rendering and hit-testing
crates/app-terminal  native local Crossterm loop
crates/app-web       Ratzilla/WebAssembly adapter and API polling
crates/app-server    Axum HTTP, SQLite, SSH, presence, and Vim-like editor
```
