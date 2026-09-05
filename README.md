# svetsec.ru

One Ratatui interface for the browser, a local terminal, and SSH. The server
adds SQLite-backed accounts, sessions, article comments, and articles.

## What is implemented

- The dot beside `svetsec.ru` is a neutral brand mark; the site does not publish
  whether the owner is currently online.
- `r` switches the language in the browser and terminal; the destination
  country's flag appears for 1.5 seconds as confirmation.
- Desktop mouse hover shows a short single-line hint in the footer.
- Browser owner login uses an Argon2id password hash and an opaque
  `HttpOnly; SameSite=Strict` cookie. Only a SHA-256 hash of the random session
  token is stored in SQLite.
- Visitors can register a username and password, log in, and leave comments on
  articles. Reader passwords use the same salted Argon2id storage, and comments
  are rate-limited server-side.
- SSH identifies the owner by username plus a verified public key. Other
  visitors can use a guest session or authenticate with a registered username
  and password to comment.
- The owner can open Articles and press `e` over SSH to enter the terminal
  editor. Article writes are enforced server-side.
- Published Markdown is discovered from `main/articles` through the server.
  Localized `title`, ISO `date`, and labels are preloaded from frontmatter, then
  the index is sorted newest-first and separated into date groups. A language
  file body is fetched only when opened. Both loading states fill the article
  panel with the shared three-well Gaussian braille animation.
- Browser history uses `/`, `/articles`, `/articles/<slug>`, `/projects`, and
  `/info`, so a refresh or Back/Forward navigation preserves the current screen.
- Every browser click target has pointer and hover feedback: menu tabs, article
  rows, project cards, code actions, and the Python output close button.
- The browser UI fills the visual viewport and uses a light palette. Open
  articles use native browser wheel/touch scrolling, expose a real scrollbar,
  and constrain text selection to the paragraph or code block where the drag
  started. They also have a visible Back action plus large touch controls on
  phones.
- Markdown images keep their original quality in the browser and use compact
  true-color previews over SSH.
- Article frontmatter provides colored labels. Python fences from the selected
  article source can be executed through the same server-side Pyodide runner
  from the browser or SSH. The right-hand article panel contains comments;
  Python output temporarily replaces it and remains there until `x` (or its
  close button) is used.
  Label matching is case-insensitive; unknown labels keep a deterministic
  palette color.
- Info links to the generated PDF resume at `/resume`. Its Typst source lives in
  `resume/resume.typ`; `.github/workflows/resume.yml` rebuilds and commits the
  web asset when that source changes.
- Projects presents `brand.tbank.ru` and the `securesvet/svetsec` repository as
  keyboard- and pointer-accessible cards shared by the browser and terminal UI.

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
deployment is no longer sufficient because sessions, comments, articles, and
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

- `Left`/`Right`, `h`/`l`, or `1`–`4`: sections
- `r`: switch language
- `a` outside an article: owner login; `a` inside an article: reader login
- `s` inside an article: register a reader account
- `m` inside an article: open comments or write one when signed in
- `d` inside an article: log out
- `j`/`k` or arrows: select an article, or scroll the open article
- `Enter`/`o`: open the selected Markdown article
- `e`: edit the selected article on GitHub; creates one if the list is empty
- `n`: create a new Markdown article on GitHub
- `f`: reload the article index from the configured source
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

Registered readers can use their username and password and press `m` in an
open article to write a comment:

```sh
ssh -p 2222 YourUsername@svetsec.ru
```

SSH owner (the key must match `SVETSEC_OWNER_PUBLIC_KEY_FILE`):

```sh
ssh -p 2222 -i ~/.ssh/id_ed25519 owner@svetsec.ru
```

In the owner session, open Articles (`2`) and press `e`. Editor commands:

- `i`, `a`, `o`: enter Insert mode
- `Esc`: return to Normal mode
- `h`, `j`, `k`, `l`, `x`: cursor movement/deletion
- `:title TEXT`, `:slug SLUG`, `:date YYYY-MM-DD`: edit metadata
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

Push public articles to both `articles/<slug>/en.md` and
`articles/<slug>/ru.md` on `main`. The list preloads localized `title`, `date`,
and labels from frontmatter, while the selected language file body is loaded
only when a visitor opens the article. A fallback remains available if one
translation is temporarily absent. Files beginning with `_` are ignored; see
`articles/_FORMAT.md` for the supported skeleton.

Articles use the repository's local `articles/` directory by default:

```sh
SVETSEC_ARTICLES_SOURCE='local'
SVETSEC_ARTICLES_DIR='articles'
```

The browser and SSH server read Markdown and images directly from this folder.
The production Docker image includes the same directory, so a push to `main`
publishes article changes atomically with the normal deployment and makes no
GitHub API requests at runtime. Article-only changes reuse the compiled
application layers during the Docker build.

Set `SVETSEC_ARTICLES_SOURCE=github` only if a deployment must read another
repository dynamically. Public repositories need no token, but this mode is
subject to GitHub API limits; private repositories require a server-side
fine-grained token with read-only Contents permission.

## API and data model

The server creates the SQLite schema automatically. It contains
`web_sessions`, `users`, `user_sessions`, `comments`, and `articles` tables and
enables WAL mode. Existing databases are migrated without deleting the former
presence table, but that table is no longer read or updated.

- `GET/POST/DELETE /api/session`: state, login, logout
- `POST /api/users`: register and start a reader session
- `POST /api/users/session`: log into a reader session
- `GET/POST /api/articles/:slug/comments`: list or add article comments
- `GET /api/articles`: published articles for guests, drafts included for owner
- `POST /api/articles`: create/update by slug; owner session required
- `GET /resume`: serve the generated PDF at the short public URL
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
crates/app-web       Ratzilla/WebAssembly adapter and browser interactions
crates/app-server    Axum HTTP, SQLite, SSH, accounts, comments, and owner editor
resume               Typst source for the PDF linked from Info
```
