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
- The owner can open Articles and press `e` over SSH to enter the terminal
  editor. Article writes are enforced server-side.
- Published Markdown is discovered from `main/articles` through the server.
  The directory list loads first; a language file body is fetched only when
  opened. While it loads, the shared dot-well animation shows three large
  moving wells.
- Browser history uses `/`, `/articles`, `/articles/<slug>`, and `/info`, so a
  refresh or Back/Forward navigation preserves the current screen.
- Every browser click target has pointer and hover feedback: menu tabs, article
  rows, code actions, and the Python output close button.
- The browser UI fills the visual viewport, uses a dark palette, and keeps
  article text selectable. Open articles have a visible Back action plus large
  touch controls on phones.
- Markdown images keep their original quality in the browser and use compact
  true-color previews over SSH.
- Article frontmatter provides colored labels. Python fences from the selected
  article source can be executed through the same server-side Pyodide runner
  from the browser or SSH. Output temporarily replaces the right-hand telemetry
  panel and remains there until `x` (or its close button) is used.
  Label matching is case-insensitive; unknown labels keep a deterministic
  palette color.
- Info links to the generated PDF resume. Its Typst source lives in
  `resume/resume.typ`; `.github/workflows/resume.yml` rebuilds and commits the
  web asset when that source changes.

## Requirements

- Stable Rust with the `wasm32-unknown-unknown` target
- [Trunk](https://trunkrs.dev/) for the browser build
- `ssh-keygen` for the SSH host key
- Node.js 18+ and `npm install` for server-side Pyodide

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
npm install
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

Copy `.env.example` to `.env` and paste the generated value into
`SVETSEC_OWNER_PASSWORD_HASH`. The server loads `.env` automatically; `.env`
is ignored by Git. Real environment variables still take precedence in
production.

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

Browser shortcuts (article navigation keys are shared with SSH):

- `Left`/`Right`, `h`/`l`, or `1`–`3`: sections
- `r`: switch language
- `a`: owner password login
- `j`/`k` or arrows: select an article, or scroll the open article
- `Enter`/`o`: open the selected Markdown article
- `e`: edit the selected article on GitHub; creates one if the list is empty
- `n`: create a new Markdown article on GitHub
- `f`: refresh the filename list after a GitHub commit
- `p`: run the focused committed `python`/`python3`/`py` fence through Pyodide
- `c`: copy the focused code block (`OSC 52` is used over SSH)
- `Esc` or the visible Back control: close the article
- `x`: close the Python output panel
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
- `h`, `j`, `k`, `l`, `x`: cursor movement/deletion
- `:title TEXT`, `:slug SLUG`: edit metadata
- `:labels cryptography, python`: set frontmatter labels (`:labels` clears them)
- `:lang en` / `:lang ru`: switch the article language buffer
- `:publish` / `:draft`: change publication state
- `:export`: write the current language buffer to
  `articles/<slug>/<en|ru>.md` so it can be committed and pushed to `main`
- `:w`, `:wq`, `:q!`: save, save-and-close, discard-and-close

Normal-mode and site shortcuts also accept the corresponding Russian-layout
keys, for example `h/р`, `j/о`, `k/л`, `l/д`, `r/к`, `e/у`, and `q/й`.
Python execution uses `p/з`.

## Markdown articles from GitHub

Push public articles to `articles/<slug>/en.md` and/or
`articles/<slug>/ru.md` on `main`. The list title is derived from the directory,
while the selected language file and its `# H1` title are loaded only when a
visitor opens the article. If that language does not exist, the server falls
back to the available translation. Files beginning with `_` are ignored; see
`articles/_FORMAT.md` for the supported skeleton.

Public repositories need no GitHub token. If the repository becomes private or
the anonymous API limit is too small, set `SVETSEC_GITHUB_TOKEN` on the server
to a fine-grained token with read-only Contents permission. Never expose it to
the WASM client.

For local development, add this to `.env`:

```sh
SVETSEC_ARTICLES_SOURCE='local'
SVETSEC_ARTICLES_DIR='articles'
```

The browser and SSH server will then read Markdown and images directly from
the local `articles/` folder. Local files bypass the GitHub/body caches, so
closing and reopening an article shows the latest saved content. Set
`SVETSEC_ARTICLES_SOURCE=github` (the default) in production to use the
configured GitHub repository again.

## API and data model

The server creates the SQLite schema automatically. It contains
`web_sessions`, `presence`, and `articles` tables and enables WAL mode.

- `GET/POST/DELETE /api/session`: state, login, logout
- `POST /api/heartbeat`: refresh owner presence
- `GET /api/articles`: published articles for guests, drafts included for owner
- `POST /api/articles`: create/update by slug; owner session required
- `GET /api/github/articles?lang=en|ru`: Markdown directory list from the
  configured source
- `GET /api/github/articles/:slug?lang=en|ru`: lazily loaded localized body
- `POST /api/github/articles/:slug/python/:block`: run one selected Python
  fence from that source article through the concurrency-, memory-, time-, and
  output-limited Pyodide worker; the request itself cannot supply source code

## GitHub Actions CI and deployment

`.github/workflows/ci.yml` checks every pull request and push to `main`.
`.github/workflows/deploy.yml` repeats the release checks, builds a production
Docker image containing the Rust server, browser `dist/`, Node.js, and the
Pyodide runtime, then transfers that image over SSH and activates it with
Docker Compose after a push to `main` (or a manual dispatch). The workflow
targets `linux/amd64`; change `DOCKER_PLATFORM` to `linux/arm64` for an ARM
server.

Create a GitHub Environment named `production`, then configure these encrypted
environment secrets:

- `DEPLOY_HOST`: server hostname or IP
- `DEPLOY_PORT`: SSH port
- `DEPLOY_USER`: restricted deployment user
- `DEPLOY_SSH_KEY`: that user's private deployment key
- `DEPLOY_KNOWN_HOSTS`: pinned `known_hosts` line for the server

Configure these environment variables (not secrets):

- `DEPLOY_PATH`: absolute release root, for example `/opt/svetsec`

Install Docker Engine with the Compose v2 plugin on the server and grant the
restricted deployment user access to that Docker daemon. Prepare the persistent
files once:

```sh
sudo mkdir -p /opt/svetsec/shared /opt/svetsec/data
sudo chown -R deploy:deploy /opt/svetsec
# /opt/svetsec/shared/.env
# /opt/svetsec/shared/owner_ed25519.pub
# /opt/svetsec/shared/ssh_host_ed25519_key
```

The Compose deployment mounts `shared/` read-only, stores SQLite in
`$DEPLOY_PATH/data/svetsec.db`, and runs Caddy on ports 80 and 443 with automatic
HTTPS for `svetsec.ru`. The application remains available on host loopback port
3000 for diagnostics, and its SSH interface is published on host port 2222.
Both application ports can be changed in `shared/.env` with
`SVETSEC_HTTP_PORT` and `SVETSEC_SSH_PORT`.
Inside the container the key paths are fixed to
`/run/svetsec/owner_ed25519.pub` and
`/run/svetsec/ssh_host_ed25519_key`, so host-specific absolute key paths are no
longer needed in `.env`.

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
crates/app-server    Axum HTTP, SQLite, SSH, presence, and owner editor
resume               Typst source for the PDF linked from Info
```
