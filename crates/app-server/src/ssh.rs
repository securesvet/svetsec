use std::{
    collections::HashMap,
    io::Write,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use russh::{
    Channel, ChannelId, Pty,
    keys::{PrivateKey, ssh_key::PublicKey},
    server::{Auth, ChannelOpenHandle, Config, Handle, Handler, Msg, Server, Session},
};
use svetsec_core::{
    App, ArticleContent, ArticleImage, ArticleSummary, HelpTarget, Language, Message, Tab,
};
use tokio::sync::{Mutex, mpsc::UnboundedSender, mpsc::unbounded_channel};

use crate::db::{ArticleInput, Database};
use crate::github::{GithubSource, valid_date};
use crate::python::PyodideRunner;

type SshTerminal = Terminal<CrosstermBackend<TerminalHandle>>;

struct TerminalHandle {
    sender: UnboundedSender<Vec<u8>>,
    sink: Vec<u8>,
}

impl TerminalHandle {
    fn start(handle: Handle, channel_id: ChannelId) -> Self {
        let (sender, mut receiver) = unbounded_channel::<Vec<u8>>();
        tokio::spawn(async move {
            while let Some(data) = receiver.recv().await {
                if handle.data(channel_id, data).await.is_err() {
                    break;
                }
            }
        });
        Self {
            sender,
            sink: Vec::new(),
        }
    }
}

impl Write for TerminalHandle {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.sink.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.sender
            .send(std::mem::take(&mut self.sink))
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::BrokenPipe, error))
    }
}

struct Client {
    terminal: SshTerminal,
    app: App,
    editor: Option<VimEditor>,
    owner: bool,
}

#[derive(Clone)]
struct SshServer {
    clients: Arc<Mutex<HashMap<usize, Client>>>,
    database: Database,
    owner_user: Arc<str>,
    owner_key: Arc<PublicKey>,
    github: GithubSource,
    profile_image: Arc<ArticleImage>,
    pyodide: PyodideRunner,
    id: usize,
    authenticated_owner: bool,
}

pub async fn serve(
    address: SocketAddr,
    database: Database,
    owner_user: String,
    owner_key: PublicKey,
    host_key: PrivateKey,
    github: GithubSource,
    pyodide: PyodideRunner,
) -> Result<()> {
    let profile = crate::github::rasterize_image_with_bounds(
        "/assets/profile.jpg",
        "Sviatoslav M.",
        include_bytes!("../../app-web/assets/profile.jpg"),
        18,
        18,
    )?;
    let mut server = SshServer {
        clients: Arc::new(Mutex::new(HashMap::new())),
        database,
        owner_user: owner_user.into(),
        owner_key: Arc::new(owner_key),
        github,
        profile_image: Arc::new(ArticleImage {
            source: profile.source,
            alt: profile.alt,
            width: profile.width,
            height: profile.height,
            pixels: profile.pixels,
        }),
        pyodide,
        id: 0,
        authenticated_owner: false,
    };
    server.start_render_loop();
    let config = Config {
        inactivity_timeout: Some(Duration::from_secs(60 * 60 * 4)),
        auth_rejection_time: Duration::from_secs(1),
        auth_rejection_time_initial: Some(Duration::ZERO),
        keys: vec![host_key],
        nodelay: true,
        ..Default::default()
    };
    tracing::info!(%address, "SSH server listening");
    server.run_on_address(Arc::new(config), address).await?;
    Ok(())
}

impl SshServer {
    fn start_render_loop(&self) {
        let clients = Arc::clone(&self.clients);
        let database = self.database.clone();
        tokio::spawn(async move {
            let mut online = false;
            let mut next_refresh = tokio::time::Instant::now();
            let mut advance_skeleton = false;
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                advance_skeleton = !advance_skeleton;
                let refresh = tokio::time::Instant::now() >= next_refresh;
                if refresh {
                    online = database.owner_online().unwrap_or(false);
                    next_refresh = tokio::time::Instant::now() + Duration::from_secs(2);
                }
                let mut clients = clients.lock().await;
                for (id, client) in clients.iter_mut() {
                    if refresh && client.owner {
                        let _ = database.touch_presence(&format!("ssh:{id}"), "ssh");
                    }
                    let _ = client
                        .app
                        .update(Message::SetOwnerOnline(online || client.owner));
                    if advance_skeleton
                        && (client.app.articles_loading() || client.app.article_loading())
                    {
                        let _ = client.app.update(Message::AdvanceSkeleton);
                    }
                    if client.app.article_animation_active() {
                        let _ = client.app.update(Message::AdvanceArticleAnimation);
                    }
                    let Client {
                        terminal,
                        app,
                        editor,
                        ..
                    } = client;
                    let _ = terminal.draw(|frame| {
                        if let Some(editor) = editor {
                            render_editor(frame, editor);
                        } else {
                            app.set_article_viewport_rows(svetsec_ui::article_viewport_rows(
                                frame.area(),
                            ));
                            svetsec_ui::render(frame, app);
                        }
                    });
                }
            }
        });
    }

    fn presence_id(&self) -> String {
        format!("ssh:{}", self.id)
    }
}

impl Server for SshServer {
    type Handler = Self;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
        let client = self.clone();
        self.id += 1;
        client
    }
}

impl Handler for SshServer {
    type Error = anyhow::Error;

    async fn auth_none(&mut self, user: &str) -> Result<Auth, Self::Error> {
        self.authenticated_owner = false;
        Ok(if user == self.owner_user.as_ref() {
            Auth::reject()
        } else {
            Auth::Accept
        })
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        let is_owner =
            user == self.owner_user.as_ref() && public_key.key_data() == self.owner_key.key_data();
        self.authenticated_owner = is_owner;
        Ok(if is_owner {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn auth_succeeded(&mut self, _: &mut Session) -> Result<(), Self::Error> {
        if self.authenticated_owner {
            self.database.touch_presence(&self.presence_id(), "ssh")?;
        }
        Ok(())
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let backend = CrosstermBackend::new(TerminalHandle::start(session.handle(), channel.id()));
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::default()),
            },
        )?;
        let mut app = App::default();
        app.set_profile_image((*self.profile_image).clone());
        let _ = app.update(Message::SetAuthenticated(self.authenticated_owner));
        let _ = app.update(Message::SetOwnerOnline(
            self.database.owner_online()? || self.authenticated_owner,
        ));
        self.clients.lock().await.insert(
            self.id,
            Client {
                terminal,
                app,
                editor: None,
                owner: self.authenticated_owner,
            },
        );
        reply.accept().await;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let mut clients = self.clients.lock().await;
        let Some(client) = clients.get_mut(&self.id) else {
            return Ok(());
        };

        if let Some(editor) = &mut client.editor {
            let outcome = editor.input(data, &self.database)?;
            if outcome == EditorOutcome::Close {
                client.editor = None;
            }
            return Ok(());
        }

        if data == b"q" || data == "й".as_bytes() || data == b"\x03" {
            clients.remove(&self.id);
            self.database.remove_presence(&self.presence_id())?;
            session.close(channel)?;
            return Ok(());
        }

        let mut load_list = false;
        let mut load_body = None;
        let mut python_code = None;
        if client.app.selected() == Tab::Articles {
            let article_open = client.app.opened_article().is_some();
            if data == b"f" || data == "а".as_bytes() {
                client.app.begin_articles_load();
                load_list = true;
            }
            if article_open
                && (data == b"x" || data == "ч".as_bytes())
                && client.app.python_output().is_some()
            {
                let _ = client.app.update(Message::DismissPythonOutput);
                return Ok(());
            }
            if article_open
                && (data == b"c" || data == "с".as_bytes())
                && let Some(block) = client.app.focused_code_block()
                && !block.animated()
                && !block.code.is_empty()
            {
                let clipboard = format!("\x1b]52;c;{}\x07", STANDARD.encode(block.code));
                session.data(channel, clipboard.into_bytes())?;
                return Ok(());
            }
            let run_key = article_open && (data == b"p" || data == "з".as_bytes());
            if run_key && !client.app.python_running() {
                python_code = client.app.python_code();
                if python_code.is_some() {
                    client.app.begin_python_run();
                }
            }
            if data == b"\x1b[A" || data == b"k" || data == "л".as_bytes() {
                let message = if client.app.opened_article().is_some() {
                    Message::ScrollArticleUp
                } else {
                    Message::PreviousArticle
                };
                let _ = client.app.update(message);
                return Ok(());
            }
            if data == b"\x1b[B" || data == b"j" || data == "о".as_bytes() {
                let message = if client.app.opened_article().is_some() {
                    Message::ScrollArticleDown
                } else {
                    Message::NextArticle
                };
                let _ = client.app.update(message);
                return Ok(());
            }
            if (data == b"\r" || data == b"o" || data == "щ".as_bytes())
                && client.app.opened_article().is_none()
                && !client.app.article_loading()
            {
                load_body = client
                    .app
                    .selected_article()
                    .map(|article| article.slug.clone());
                if load_body.is_some() {
                    client.app.begin_article_load();
                }
            }
            if data == b"\x1b" && article_open {
                let _ = client.app.update(Message::CloseArticle);
                return Ok(());
            }
        }

        if client.app.selected() == Tab::Projects {
            if data == b"\x1b[A" || data == b"k" || data == "л".as_bytes() {
                let _ = client.app.update(Message::PreviousProject);
                return Ok(());
            }
            if data == b"\x1b[B" || data == b"j" || data == "о".as_bytes() {
                let _ = client.app.update(Message::NextProject);
                return Ok(());
            }
            if data == b"\r" || data == b"o" || data == "щ".as_bytes() {
                let clipboard = format!(
                    "\x1b]52;c;{}\x07",
                    STANDARD.encode(client.app.selected_project().url)
                );
                session.data(channel, clipboard.into_bytes())?;
                return Ok(());
            }
        }

        let message = if data == b"\x1b[C" || data == b"l" || data == "д".as_bytes() {
            Some(Message::NextTab)
        } else if data == b"\x1b[D" || data == b"h" || data == "р".as_bytes() {
            Some(Message::PreviousTab)
        } else if data == b"1" {
            Some(Message::SelectTab(Tab::Main))
        } else if data == b"2" {
            Some(Message::SelectTab(Tab::Articles))
        } else if data == b"3" {
            Some(Message::SelectTab(Tab::Projects))
        } else if data == b"4" {
            Some(Message::SelectTab(Tab::Info))
        } else if data == b"r" || data == "к".as_bytes() {
            Some(Message::ToggleLanguage)
        } else if data == b"?" {
            Some(Message::Hover(Some(HelpTarget::Logo)))
        } else {
            None
        };
        if let Some(message) = message {
            let _ = client.app.update(message);
        }
        if (data == b"r" || data == "к".as_bytes()) && client.app.selected() == Tab::Articles {
            if let Some(slug) = client
                .app
                .opened_article()
                .map(|article| article.slug.clone())
            {
                client.app.begin_article_load();
                load_body = Some(slug);
            } else {
                client.app.begin_articles_load();
                load_list = true;
            }
        }
        if client.app.selected() == Tab::Articles
            && !client.app.articles_loaded()
            && !client.app.articles_loading()
        {
            client.app.begin_articles_load();
            load_list = true;
        }
        if data == b"r" || data == "к".as_bytes() {
            let generation = client.app.language_notice_generation();
            let clients = Arc::clone(&self.clients);
            let id = self.id;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1_500)).await;
                if let Some(client) = clients.lock().await.get_mut(&id) {
                    let _ = client.app.update(Message::HideLanguageNotice(generation));
                }
            });
        }
        if (data == b"e" || data == "у".as_bytes())
            && client.owner
            && client.app.selected() == Tab::Articles
        {
            client.editor = Some(VimEditor::new(client.app.language()));
        }
        drop(clients);
        if load_list {
            self.load_github_articles(data == b"f" || data == "а".as_bytes());
        }
        if let Some(slug) = load_body {
            self.load_github_article(slug);
        }
        if let Some(code) = python_code {
            self.run_python(code);
        }
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _: ChannelId,
        col_width: u32,
        row_height: u32,
        _: u32,
        _: u32,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.resize(col_width, row_height).await
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _: &str,
        col_width: u32,
        row_height: u32,
        _: u32,
        _: u32,
        _: &[(Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.resize(col_width, row_height).await?;
        session.channel_success(channel)?;
        Ok(())
    }
}

impl SshServer {
    fn load_github_articles(&self, refresh: bool) {
        let github = self.github.clone();
        let clients = Arc::clone(&self.clients);
        let id = self.id;
        tokio::spawn(async move {
            let language = clients
                .lock()
                .await
                .get(&id)
                .map_or(Language::default(), |client| client.app.language());
            let result = github.list(refresh, language).await;
            if let Some(client) = clients.lock().await.get_mut(&id) {
                match result {
                    Ok(articles) => client.app.set_articles(
                        articles
                            .into_iter()
                            .map(|article| ArticleSummary {
                                slug: article.slug,
                                title_en: article.title_en,
                                title_ru: article.title_ru,
                                date: article.date,
                                published: article.published,
                                source_path: Some(article.source_path),
                                edit_url: Some(article.edit_url),
                                labels: article.labels,
                            })
                            .collect(),
                    ),
                    Err(_) => client.app.set_articles_error("Could not load articles."),
                }
            }
        });
    }

    fn load_github_article(&self, slug: String) {
        let github = self.github.clone();
        let clients = Arc::clone(&self.clients);
        let id = self.id;
        tokio::spawn(async move {
            let language = clients
                .lock()
                .await
                .get(&id)
                .map_or(Language::default(), |client| client.app.language());
            let result = github.article(&slug, language).await;
            if let Some(client) = clients.lock().await.get_mut(&id) {
                match result {
                    Ok(article) => client.app.set_opened_article(ArticleContent {
                        slug: article.slug,
                        title: article.title,
                        markdown: article.markdown,
                        images: article
                            .images
                            .into_iter()
                            .map(|image| ArticleImage {
                                source: image.source,
                                alt: image.alt,
                                width: image.width,
                                height: image.height,
                                pixels: image.pixels,
                            })
                            .collect(),
                        labels: article.labels,
                    }),
                    Err(_) => client
                        .app
                        .set_articles_error("Could not load this Markdown file."),
                }
            }
        });
    }

    fn run_python(&self, code: String) {
        let pyodide = self.pyodide.clone();
        let clients = Arc::clone(&self.clients);
        let id = self.id;
        tokio::spawn(async move {
            let result = pyodide.run(&code).await;
            if let Some(client) = clients.lock().await.get_mut(&id) {
                client.app.finish_python_run(match result {
                    Ok(output) if output.trim().is_empty() => "(no output)".into(),
                    Ok(output) => output,
                    Err(error) => format!("Python unavailable: {error}"),
                });
            }
        });
    }

    async fn resize(&self, col_width: u32, row_height: u32) -> Result<()> {
        if let Some(client) = self.clients.lock().await.get_mut(&self.id) {
            client.terminal.resize(Rect::new(
                0,
                0,
                col_width.min(u16::MAX.into()) as u16,
                row_height.min(u16::MAX.into()) as u16,
            ))?;
        }
        Ok(())
    }
}

impl Drop for SshServer {
    fn drop(&mut self) {
        if self.authenticated_owner {
            let _ = self.database.remove_presence(&self.presence_id());
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditorMode {
    Normal,
    Insert,
    Command,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditorOutcome {
    Continue,
    Close,
}

struct VimEditor {
    buffers: [Vec<String>; 2],
    cursor_row: usize,
    cursor_col: usize,
    language: Language,
    mode: EditorMode,
    command: String,
    slug: String,
    titles: [String; 2],
    date: String,
    labels: Vec<String>,
    published: bool,
    dirty: bool,
    status: String,
}

impl VimEditor {
    fn new(language: Language) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            buffers: [vec![String::new()], vec![String::new()]],
            cursor_row: 0,
            cursor_col: 0,
            language,
            mode: EditorMode::Normal,
            command: String::new(),
            slug: format!("draft-{timestamp}"),
            titles: ["Untitled".into(), "Без названия".into()],
            date: iso_date_from_unix(timestamp),
            labels: Vec::new(),
            published: false,
            dirty: false,
            status: "NORMAL  i insert · :w save · :help commands".into(),
        }
    }

    fn input(&mut self, data: &[u8], database: &Database) -> Result<EditorOutcome> {
        if data == b"\x1b" {
            self.mode = EditorMode::Normal;
            self.command.clear();
            self.status = "NORMAL".into();
            return Ok(EditorOutcome::Continue);
        }
        let text = String::from_utf8_lossy(data);
        for character in text.chars() {
            let outcome = match self.mode {
                EditorMode::Normal => self.normal(character),
                EditorMode::Insert => self.insert(character),
                EditorMode::Command => self.command(character, database)?,
            };
            if outcome == EditorOutcome::Close {
                return Ok(outcome);
            }
        }
        Ok(EditorOutcome::Continue)
    }

    fn normal(&mut self, character: char) -> EditorOutcome {
        match character {
            'i' | 'ш' => self.mode = EditorMode::Insert,
            'a' | 'ф' => {
                self.cursor_col = (self.cursor_col + 1).min(self.current_line_len());
                self.mode = EditorMode::Insert;
            }
            'o' | 'щ' => {
                self.cursor_row += 1;
                self.cursor_col = 0;
                let row = self.cursor_row;
                self.lines_mut().insert(row, String::new());
                self.mode = EditorMode::Insert;
                self.dirty = true;
            }
            'h' | 'р' => self.cursor_col = self.cursor_col.saturating_sub(1),
            'l' | 'д' => self.cursor_col = (self.cursor_col + 1).min(self.current_line_len()),
            'j' | 'о' => self.move_row(1),
            'k' | 'л' => self.move_row(-1),
            'x' | 'ч' => self.delete_at_cursor(),
            ':' | 'Ж' | 'ж' => {
                self.command.clear();
                self.mode = EditorMode::Command;
            }
            _ => {}
        }
        self.status = match self.mode {
            EditorMode::Normal => "NORMAL".into(),
            EditorMode::Insert => "INSERT  Esc normal".into(),
            EditorMode::Command => ":".into(),
        };
        EditorOutcome::Continue
    }

    fn insert(&mut self, character: char) -> EditorOutcome {
        match character {
            '\u{7f}' | '\u{8}' => self.backspace(),
            '\r' | '\n' => {
                let col = self.cursor_col;
                let row = self.cursor_row;
                let byte = char_byte_index(&self.lines()[row], col);
                let tail = self.lines_mut()[row].split_off(byte);
                self.cursor_row += 1;
                self.cursor_col = 0;
                let row = self.cursor_row;
                self.lines_mut().insert(row, tail);
                self.dirty = true;
            }
            character if !character.is_control() => {
                let row = self.cursor_row;
                let byte = char_byte_index(&self.lines()[row], self.cursor_col);
                self.lines_mut()[row].insert(byte, character);
                self.cursor_col += 1;
                self.dirty = true;
            }
            _ => {}
        }
        EditorOutcome::Continue
    }

    fn command(&mut self, character: char, database: &Database) -> Result<EditorOutcome> {
        if character == '\r' || character == '\n' {
            let command = std::mem::take(&mut self.command);
            self.mode = EditorMode::Normal;
            return self.run_command(command.trim(), database);
        }
        if character == '\u{7f}' || character == '\u{8}' {
            self.command.pop();
        } else if !character.is_control() {
            self.command.push(command_key(character));
        }
        self.status = format!(":{}", self.command);
        Ok(EditorOutcome::Continue)
    }

    fn run_command(&mut self, command: &str, database: &Database) -> Result<EditorOutcome> {
        match command {
            "w" => self.save(database)?,
            "wq" => {
                self.save(database)?;
                return Ok(EditorOutcome::Close);
            }
            "export" => self.export_markdown()?,
            "q" if !self.dirty => return Ok(EditorOutcome::Close),
            "q" => self.status = "Unsaved changes; use :w or :q!".into(),
            "q!" => return Ok(EditorOutcome::Close),
            "publish" => {
                self.published = true;
                self.dirty = true;
                self.status = "Article will be published on :w".into();
            }
            "draft" => {
                self.published = false;
                self.dirty = true;
                self.status = "Article is a draft".into();
            }
            "lang en" => self.switch_language(Language::En),
            "lang ru" => self.switch_language(Language::Ru),
            "labels" => {
                self.labels.clear();
                self.dirty = true;
                self.status = "Labels cleared".into();
            }
            "help" => {
                self.status =
                    ":title TEXT · :date YYYY-MM-DD · :labels A,B · :lang en|ru · :export · :wq"
                        .into();
            }
            _ if command.starts_with("title ") => {
                let index = self.language_index();
                self.titles[index] = command[6..].trim().to_owned();
                self.dirty = true;
                self.status = "Title updated".into();
            }
            _ if command.starts_with("slug ") => {
                self.slug = command[5..].trim().to_owned();
                self.dirty = true;
                self.status = "Slug updated".into();
            }
            _ if command.starts_with("date ") => {
                let date = command[5..].trim();
                if valid_date(date) {
                    self.date = date.to_owned();
                    self.dirty = true;
                    self.status = "Publication date updated".into();
                } else {
                    self.status = "Date must use YYYY-MM-DD".into();
                }
            }
            _ if command.starts_with("labels ") => match editor_labels(command[7..].trim()) {
                Ok(labels) => {
                    self.labels = labels;
                    self.dirty = true;
                    self.status = format!("Labels: {}", self.labels.join(", "));
                }
                Err(error) => self.status = error.to_string(),
            },
            _ => self.status = format!("Not an editor command: {command}"),
        }
        Ok(EditorOutcome::Continue)
    }

    fn save(&mut self, database: &Database) -> Result<()> {
        if !valid_article_slug(&self.slug) {
            anyhow::bail!("slug must contain only ASCII letters, numbers, - or _");
        }
        let article = ArticleInput {
            slug: self.slug.clone(),
            title_en: self.titles[0].clone(),
            title_ru: self.titles[1].clone(),
            body_en: self.buffers[0].join("\n"),
            body_ru: self.buffers[1].join("\n"),
            published: self.published,
        };
        database.save_article(&article)?;
        self.dirty = false;
        self.status = format!("Written: {}", self.slug);
        Ok(())
    }

    fn export_markdown(&mut self) -> Result<()> {
        if !valid_article_slug(&self.slug) {
            anyhow::bail!("slug must contain only ASCII letters, numbers, - or _");
        }
        let directory = std::env::var("SVETSEC_ARTICLES_DIR").unwrap_or_else(|_| "articles".into());
        let directory = std::path::Path::new(&directory).join(&self.slug);
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{}.md", self.language.path_code()));
        let labels = if self.labels.is_empty() {
            String::new()
        } else {
            let labels = self
                .labels
                .iter()
                .map(|label| serde_json::to_string(label).map(|label| format!("  - {label}")))
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            format!("labels:\n{labels}\n")
        };
        let title = serde_json::to_string(&self.titles[self.language_index()])?;
        let frontmatter = format!("---\ntitle: {title}\ndate: {}\n{labels}---\n\n", self.date);
        let markdown = format!(
            "{frontmatter}# {}\n\n{}\n",
            self.titles[self.language_index()],
            self.lines().join("\n")
        );
        std::fs::write(&path, markdown)?;
        self.status = format!("Exported: {}", path.display());
        Ok(())
    }

    fn switch_language(&mut self, language: Language) {
        self.language = language;
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.status = format!("Editing {}", language.code());
    }

    fn lines(&self) -> &Vec<String> {
        &self.buffers[self.language_index()]
    }

    fn lines_mut(&mut self) -> &mut Vec<String> {
        let index = self.language_index();
        &mut self.buffers[index]
    }

    fn language_index(&self) -> usize {
        match self.language {
            Language::En => 0,
            Language::Ru => 1,
        }
    }

    fn current_line_len(&self) -> usize {
        self.lines()[self.cursor_row].chars().count()
    }

    fn move_row(&mut self, delta: isize) {
        self.cursor_row = self
            .cursor_row
            .saturating_add_signed(delta)
            .min(self.lines().len().saturating_sub(1));
        self.cursor_col = self.cursor_col.min(self.current_line_len());
    }

    fn delete_at_cursor(&mut self) {
        let row = self.cursor_row;
        if self.cursor_col < self.current_line_len() {
            let start = char_byte_index(&self.lines()[row], self.cursor_col);
            let end = char_byte_index(&self.lines()[row], self.cursor_col + 1);
            self.lines_mut()[row].replace_range(start..end, "");
            self.dirty = true;
        }
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let row = self.cursor_row;
            let start = char_byte_index(&self.lines()[row], self.cursor_col - 1);
            let end = char_byte_index(&self.lines()[row], self.cursor_col);
            self.lines_mut()[row].replace_range(start..end, "");
            self.cursor_col -= 1;
            self.dirty = true;
        } else if self.cursor_row > 0 {
            let row = self.cursor_row;
            let current = self.lines_mut().remove(row);
            self.cursor_row -= 1;
            self.cursor_col = self.current_line_len();
            let row = self.cursor_row;
            self.lines_mut()[row].push_str(&current);
            self.dirty = true;
        }
    }
}

fn char_byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map_or(text.len(), |(index, _)| index)
}

fn valid_article_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn iso_date_from_unix(timestamp: u64) -> String {
    let days = (timestamp / 86_400) as i64;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn editor_labels(value: &str) -> Result<Vec<String>> {
    let mut labels = Vec::new();
    for label in value
        .split(',')
        .map(str::trim)
        .filter(|label| !label.is_empty())
    {
        if label.chars().count() > 24 || label.chars().any(char::is_control) {
            anyhow::bail!("Each label must contain at most 24 printable characters");
        }
        if !labels
            .iter()
            .any(|existing: &String| existing.to_lowercase() == label.to_lowercase())
        {
            labels.push(label.to_owned());
        }
        if labels.len() > 6 {
            anyhow::bail!("An article can have at most 6 labels");
        }
    }
    if labels.is_empty() {
        anyhow::bail!("Use :labels A,B or :labels to clear labels");
    }
    Ok(labels)
}

fn command_key(character: char) -> char {
    match character {
        'й' => 'q',
        'ц' => 'w',
        'у' => 'e',
        'к' => 'r',
        'е' => 't',
        'н' => 'y',
        'г' => 'u',
        'ш' => 'i',
        'щ' => 'o',
        'з' => 'p',
        'ф' => 'a',
        'ы' => 's',
        'в' => 'd',
        'а' => 'f',
        'п' => 'g',
        'р' => 'h',
        'о' => 'j',
        'л' => 'k',
        'д' => 'l',
        'я' => 'z',
        'ч' => 'x',
        'с' => 'c',
        'м' => 'v',
        'и' => 'b',
        'т' => 'n',
        'ь' => 'm',
        other => other,
    }
}

fn render_editor(frame: &mut ratatui::Frame<'_>, editor: &VimEditor) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    let rows = Layout::vertical([Constraint::Min(3), Constraint::Length(2)]).split(area);
    let visible_height = rows[0].height.saturating_sub(2) as usize;
    let scroll = editor
        .cursor_row
        .saturating_sub(visible_height.saturating_sub(1));
    let lines = editor
        .lines()
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(number, line)| {
            Line::from(vec![
                ratatui::text::Span::styled(
                    format!("{:>4} ", number + 1),
                    Style::new().fg(Color::DarkGray),
                ),
                ratatui::text::Span::raw(line.clone()),
            ])
        })
        .collect::<Vec<_>>();
    let dirty = if editor.dirty { " [+]" } else { "" };
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::new().borders(Borders::ALL).title(format!(
                " {} · {} {}{} ",
                editor.slug,
                editor.language.flag(),
                editor.language.code(),
                dirty
            )))
            .wrap(Wrap { trim: false }),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(editor.status.clone()).style(Style::new().add_modifier(Modifier::BOLD)),
            Line::from(format!(
                "{} · {}",
                editor.titles[editor.language_index()],
                if editor.published {
                    "PUBLISHED"
                } else {
                    "DRAFT"
                }
            )),
        ]),
        rows[1],
    );
    if rows[0].width > 7 && rows[0].height > 2 {
        frame.set_cursor_position((
            rows[0]
                .x
                .saturating_add(6)
                .saturating_add(editor.cursor_col as u16)
                .min(rows[0].right().saturating_sub(2)),
            rows[0]
                .y
                .saturating_add(1)
                .saturating_add(editor.cursor_row.saturating_sub(scroll) as u16)
                .min(rows[0].bottom().saturating_sub(2)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::{EditorMode, EditorOutcome, VimEditor, editor_labels, iso_date_from_unix};
    use crate::db::Database;
    use svetsec_core::Language;

    #[test]
    fn owner_editor_supports_vim_insert_and_write() {
        let database = Database::open(":memory:").expect("database");
        let mut editor = VimEditor::new(Language::Ru);
        editor
            .input("ш".as_bytes(), &database)
            .expect("insert mode in Russian layout");
        assert_eq!(editor.mode, EditorMode::Insert);
        editor
            .input("Привет".as_bytes(), &database)
            .expect("unicode input");
        editor.input(b"\x1b", &database).expect("normal mode");
        editor
            .input("Жц\r".as_bytes(), &database)
            .expect("write in Russian layout");
        assert_eq!(database.list_articles(true).expect("articles").len(), 1);
        assert_eq!(
            editor.input(b":q\r", &database).expect("quit"),
            EditorOutcome::Close
        );
    }

    #[test]
    fn editor_labels_are_case_insensitive_and_bounded() {
        assert_eq!(
            editor_labels("Cryptography, python, CRYPTOGRAPHY").expect("labels"),
            ["Cryptography", "python"]
        );
        assert_eq!(
            editor_labels("Криптография, КРИПТОГРАФИЯ").expect("Unicode labels"),
            ["Криптография"]
        );
        assert!(editor_labels("one,two,three,four,five,six,seven").is_err());
        assert_eq!(iso_date_from_unix(0), "1970-01-01");
        assert_eq!(iso_date_from_unix(1_788_134_400), "2026-08-31");
    }
}
