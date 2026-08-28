pub const SITE_URL: &str = "https://svetsec.ru";
pub const DOT_WELL_LANGUAGE: &str = "dot-well";
pub const DOT_WELL_ROWS: u16 = 13;
pub const DOT_WELL_FRAMES: u16 = 120;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Project {
    pub name: &'static str,
    pub url: &'static str,
    pub display_url: &'static str,
    pub description_en: &'static str,
    pub description_ru: &'static str,
    pub tags: &'static [&'static str],
}

impl Project {
    #[must_use]
    pub const fn description(self, language: Language) -> &'static str {
        match language {
            Language::En => self.description_en,
            Language::Ru => self.description_ru,
        }
    }
}

pub const PROJECTS: [Project; 2] = [
    Project {
        name: "T-Bank brand portal",
        url: "https://brand.tbank.ru/",
        display_url: "brand.tbank.ru",
        description_en: "Guidelines for T-Bank brand, graphics, interfaces, and content.",
        description_ru: "Гайдлайны Т-Банка по бренду, графике, интерфейсам и контенту.",
        tags: &["Docusaurus", "Design system"],
    },
    Project {
        name: "svetsec",
        url: "https://github.com/securesvet/svetsec",
        display_url: "github.com/securesvet/svetsec",
        description_en: "This site's Rust TUI, shared by the browser and SSH.",
        description_ru: "Rust TUI этого сайта, общий для браузера и SSH.",
        tags: &["Rust", "WASM", "SSH"],
    },
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Language {
    #[default]
    En,
    Ru,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::En, Self::Ru];

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "EN",
            Self::Ru => "RU",
        }
    }

    #[must_use]
    pub const fn flag(self) -> &'static str {
        match self {
            Self::En => "🇺🇸",
            Self::Ru => "🇷🇺",
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::En => Self::Ru,
            Self::Ru => Self::En,
        }
    }

    #[must_use]
    pub fn from_code(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "en" => Some(Self::En),
            "ru" => Some(Self::Ru),
            _ => None,
        }
    }

    #[must_use]
    pub const fn path_code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ru => "ru",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Tab {
    #[default]
    Main,
    Articles,
    Projects,
    Info,
}

impl Tab {
    pub const ALL: [Self; 4] = [Self::Main, Self::Articles, Self::Projects, Self::Info];

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Main => 0,
            Self::Articles => 1,
            Self::Projects => 2,
            Self::Info => 3,
        }
    }

    #[must_use]
    pub const fn label(self, language: Language) -> &'static str {
        match (self, language) {
            (Self::Main, Language::En) => "Main",
            (Self::Articles, Language::En) => "Articles",
            (Self::Projects, Language::En) => "Projects",
            (Self::Info, Language::En) => "Info",
            (Self::Main, Language::Ru) => "Главная",
            (Self::Articles, Language::Ru) => "Статьи",
            (Self::Projects, Language::Ru) => "Проекты",
            (Self::Info, Language::Ru) => "О сайте",
        }
    }

    #[must_use]
    pub const fn title(self, language: Language) -> &'static str {
        match (self, language) {
            (Self::Main, Language::En) => "Hello, internet.",
            (Self::Articles, Language::En) => "Articles",
            (Self::Projects, Language::En) => "Selected projects",
            (Self::Info, Language::En) => "About svetsec.ru",
            (Self::Main, Language::Ru) => "Привет, интернет.",
            (Self::Articles, Language::Ru) => "Статьи",
            (Self::Projects, Language::Ru) => "Избранные проекты",
            (Self::Info, Language::Ru) => "О svetsec.ru",
        }
    }

    #[must_use]
    pub const fn description(self, language: Language) -> &'static str {
        match (self, language) {
            (Self::Main, Language::En) => {
                "A small corner of the internet, running from the same Rust code in your terminal and browser."
            }
            (Self::Articles, Language::En) => {
                "Notes about security, Rust, and systems. The owner can press e to write."
            }
            (Self::Projects, Language::En) => {
                "Things I have built and the source code behind this site."
            }
            (Self::Info, Language::En) => {
                "svetsec.ru is a personal site built as a cross-platform TUI experiment."
            }
            (Self::Main, Language::Ru) => {
                "Небольшой уголок интернета на одном Rust-коде для терминала и браузера."
            }
            (Self::Articles, Language::Ru) => {
                "Заметки о безопасности, Rust и системах. Владелец может нажать e для записи."
            }
            (Self::Projects, Language::Ru) => "Мои проекты и исходный код этого сайта.",
            (Self::Info, Language::Ru) => {
                "svetsec.ru — персональный сайт и эксперимент с кроссплатформенным TUI."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HelpTarget {
    Logo,
    Tab(Tab),
    Language(Language),
    Status,
    Articles,
    Article(usize),
    ArticleBack,
    Project(usize),
    Resume,
    CodeRun(usize),
    CodeCopy(usize),
    PythonOutputClose,
}

impl HelpTarget {
    #[must_use]
    pub const fn text(self, language: Language, owner_online: bool) -> &'static str {
        match (self, language, owner_online) {
            (Self::Logo, Language::En, true) => "Green means the site owner is here now.",
            (Self::Logo, Language::En, false) => "The dot turns green when the site owner is here.",
            (Self::Logo, Language::Ru, true) => "Зелёная точка: владелец сайта сейчас здесь.",
            (Self::Logo, Language::Ru, false) => {
                "Точка станет зелёной, когда владелец будет на сайте."
            }
            (Self::Tab(_), Language::En, _) => "Open this section.",
            (Self::Tab(_), Language::Ru, _) => "Открыть этот раздел.",
            (Self::Language(Language::En), Language::En, _) => "Switch to English.",
            (Self::Language(Language::Ru), Language::En, _) => "Switch to Russian.",
            (Self::Language(Language::En), Language::Ru, _) => "Переключить на английский.",
            (Self::Language(Language::Ru), Language::Ru, _) => "Переключить на русский.",
            (Self::Status, Language::En, _) => "Live session and connection state.",
            (Self::Status, Language::Ru, _) => "Состояние сессии и подключения.",
            (Self::Articles, Language::En, _) => "Owner-only article workspace.",
            (Self::Articles, Language::Ru, _) => "Редактор статей, доступный только владельцу.",
            (Self::Article(_), Language::En, _) => "Open this article.",
            (Self::Article(_), Language::Ru, _) => "Открыть эту статью.",
            (Self::ArticleBack, Language::En, _) => "Back to all articles.",
            (Self::ArticleBack, Language::Ru, _) => "Вернуться к списку статей.",
            (Self::Project(_), Language::En, _) => "Open this project.",
            (Self::Project(_), Language::Ru, _) => "Открыть этот проект.",
            (Self::Resume, Language::En, _) => "Open the PDF resume.",
            (Self::Resume, Language::Ru, _) => "Открыть резюме в PDF.",
            (Self::CodeRun(_), Language::En, _) => "Run this Python block.",
            (Self::CodeRun(_), Language::Ru, _) => "Запустить этот Python-блок.",
            (Self::CodeCopy(_), Language::En, _) => "Copy this code block.",
            (Self::CodeCopy(_), Language::Ru, _) => "Скопировать этот блок кода.",
            (Self::PythonOutputClose, Language::En, _) => "Close Python output.",
            (Self::PythonOutputClose, Language::Ru, _) => "Закрыть вывод Python.",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Message {
    NextTab,
    PreviousTab,
    SelectTab(Tab),
    SelectLanguage(Language),
    ToggleLanguage,
    SetOwnerOnline(bool),
    SetAuthenticated(bool),
    Hover(Option<HelpTarget>),
    HideLanguageNotice(u64),
    NextArticle,
    PreviousArticle,
    SelectArticle(usize),
    NextProject,
    PreviousProject,
    SelectProject(usize),
    OpenSelectedProject,
    SelectArticleCursor(u16),
    SelectArticlePosition { row: u16, column: u16 },
    ScrollArticleDown,
    ScrollArticleUp,
    MoveArticleCursorLeft,
    MoveArticleCursorRight,
    MoveArticleCursorToLineStart,
    MoveArticleCursorToLineEnd,
    MoveArticleCursorToDocumentStart,
    MoveArticleCursorToDocumentEnd,
    DismissPythonOutput,
    BeginArticleG,
    CompleteArticleG,
    CloseArticle,
    AdvanceSkeleton,
    AdvanceArticleAnimation,
    BeginSiteShortcut,
    CompleteSiteShortcut,
    CancelShortcut,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    OpenUrl(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArticleSummary {
    pub slug: String,
    pub title_en: String,
    pub title_ru: String,
    pub published: bool,
    pub source_path: Option<String>,
    pub edit_url: Option<String>,
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArticleImage {
    pub source: String,
    pub alt: String,
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArticleContent {
    pub slug: String,
    pub title: String,
    pub markdown: String,
    pub images: Vec<ArticleImage>,
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownCodeBlock {
    pub index: usize,
    pub language: String,
    pub code: String,
}

impl MarkdownCodeBlock {
    #[must_use]
    pub fn executable(&self) -> bool {
        matches!(self.language.as_str(), "python" | "python3" | "py")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArticleCodeBlock {
    pub index: usize,
    pub language: String,
    pub code: String,
    pub start_row: u16,
    pub end_row: u16,
}

impl ArticleCodeBlock {
    #[must_use]
    pub fn executable(&self) -> bool {
        matches!(self.language.as_str(), "python" | "python3" | "py")
    }

    #[must_use]
    pub fn animated(&self) -> bool {
        self.language == DOT_WELL_LANGUAGE
    }
}

struct ArticleNavigation {
    total_rows: u16,
    cursor_start: u16,
    code_blocks: Vec<ArticleCodeBlock>,
    row_widths: Vec<u16>,
}

impl ArticleSummary {
    #[must_use]
    pub fn title(&self, language: Language) -> &str {
        match language {
            Language::En => &self.title_en,
            Language::Ru => &self.title_ru,
        }
    }
}

#[derive(Debug, Default)]
pub struct App {
    selected: Tab,
    language: Language,
    owner_online: bool,
    authenticated: bool,
    articles: Vec<ArticleSummary>,
    articles_loaded: bool,
    articles_loading: bool,
    article_loading: bool,
    selected_article: usize,
    selected_project: usize,
    opened_article: Option<ArticleContent>,
    articles_error: Option<String>,
    article_create_url: Option<String>,
    profile_image: Option<ArticleImage>,
    python_running: bool,
    python_output: Option<String>,
    skeleton_phase: u8,
    article_animation_phase: u16,
    article_scroll: u16,
    article_scroll_limit: u16,
    article_viewport_rows: u16,
    article_cursor: u16,
    article_cursor_column: u16,
    article_preferred_column: u16,
    language_notice: bool,
    language_notice_generation: u64,
    hovered: Option<HelpTarget>,
    awaiting_site_key: bool,
    awaiting_article_g: bool,
    should_quit: bool,
}

impl App {
    #[must_use]
    pub const fn selected(&self) -> Tab {
        self.selected
    }

    #[must_use]
    pub const fn language(&self) -> Language {
        self.language
    }

    pub fn restore_language(&mut self, language: Language) {
        self.language = language;
        self.language_notice = false;
    }

    #[must_use]
    pub const fn owner_online(&self) -> bool {
        self.owner_online
    }

    #[must_use]
    pub const fn authenticated(&self) -> bool {
        self.authenticated
    }

    #[must_use]
    pub fn articles(&self) -> &[ArticleSummary] {
        &self.articles
    }

    #[must_use]
    pub const fn language_notice(&self) -> bool {
        self.language_notice
    }

    #[must_use]
    pub const fn language_notice_generation(&self) -> u64 {
        self.language_notice_generation
    }

    pub fn set_articles(&mut self, articles: Vec<ArticleSummary>) {
        self.articles = articles;
        self.articles_loaded = true;
        self.articles_loading = false;
        self.articles_error = None;
        self.selected_article = self
            .selected_article
            .min(self.articles.len().saturating_sub(1));
    }

    pub fn set_article_create_url(&mut self, url: String) {
        self.article_create_url = Some(url);
    }

    pub fn set_profile_image(&mut self, image: ArticleImage) {
        self.profile_image = Some(image);
    }

    #[must_use]
    pub const fn profile_image(&self) -> Option<&ArticleImage> {
        self.profile_image.as_ref()
    }

    #[must_use]
    pub fn python_code(&self) -> Option<String> {
        let block = self.focused_code_block()?;
        block.executable().then_some(block.code)
    }

    #[must_use]
    pub fn focused_code_block(&self) -> Option<ArticleCodeBlock> {
        let cursor = self.article_cursor;
        self.article_navigation()?
            .code_blocks
            .into_iter()
            .find(|block| (block.start_row..=block.end_row).contains(&cursor))
    }

    #[must_use]
    pub fn article_code_block(&self, index: usize) -> Option<ArticleCodeBlock> {
        self.article_navigation()?
            .code_blocks
            .into_iter()
            .find(|block| block.index == index)
    }

    #[must_use]
    pub const fn article_cursor(&self) -> u16 {
        self.article_cursor
    }

    #[must_use]
    pub const fn article_cursor_column(&self) -> u16 {
        self.article_cursor_column
    }

    #[must_use]
    pub fn article_line_width(&self, row: u16) -> u16 {
        self.article_navigation()
            .and_then(|layout| layout.row_widths.get(usize::from(row)).copied())
            .unwrap_or(1)
            .max(1)
    }

    #[must_use]
    pub const fn article_scroll_limit(&self) -> u16 {
        self.article_scroll_limit
    }

    #[must_use]
    pub fn article_total_rows(&self) -> u16 {
        self.article_navigation()
            .map_or(0, |layout| layout.total_rows)
    }

    pub fn set_article_viewport_rows(&mut self, rows: u16) {
        self.article_viewport_rows = rows.max(1);
        self.refresh_article_bounds();
    }

    #[must_use]
    pub const fn python_running(&self) -> bool {
        self.python_running
    }

    #[must_use]
    pub fn python_output(&self) -> Option<&str> {
        self.python_output.as_deref()
    }

    pub fn begin_python_run(&mut self) {
        self.python_running = true;
        self.python_output = None;
        self.refresh_article_bounds();
    }

    pub fn finish_python_run(&mut self, output: impl Into<String>) {
        self.python_running = false;
        self.python_output = Some(output.into());
        self.refresh_article_bounds();
    }

    #[must_use]
    pub fn article_create_url(&self) -> Option<&str> {
        self.article_create_url.as_deref()
    }

    #[must_use]
    pub fn article_editor_url(&self) -> Option<&str> {
        self.selected_article()
            .and_then(|article| article.edit_url.as_deref())
            .or(self.article_create_url.as_deref())
    }

    #[must_use]
    pub const fn articles_loaded(&self) -> bool {
        self.articles_loaded
    }

    #[must_use]
    pub const fn articles_loading(&self) -> bool {
        self.articles_loading
    }

    #[must_use]
    pub const fn article_loading(&self) -> bool {
        self.article_loading
    }

    #[must_use]
    pub const fn selected_article_index(&self) -> usize {
        self.selected_article
    }

    #[must_use]
    pub fn selected_article(&self) -> Option<&ArticleSummary> {
        self.articles.get(self.selected_article)
    }

    #[must_use]
    pub const fn selected_project_index(&self) -> usize {
        self.selected_project
    }

    #[must_use]
    pub fn selected_project(&self) -> Project {
        PROJECTS[self.selected_project.min(PROJECTS.len().saturating_sub(1))]
    }

    #[must_use]
    pub const fn opened_article(&self) -> Option<&ArticleContent> {
        self.opened_article.as_ref()
    }

    #[must_use]
    pub fn articles_error(&self) -> Option<&str> {
        self.articles_error.as_deref()
    }

    #[must_use]
    pub const fn skeleton_phase(&self) -> u8 {
        self.skeleton_phase
    }

    #[must_use]
    pub const fn article_animation_phase(&self) -> u16 {
        self.article_animation_phase
    }

    #[must_use]
    pub fn article_has_animation(&self) -> bool {
        self.opened_article.as_ref().is_some_and(|article| {
            markdown_code_blocks(&article.markdown)
                .iter()
                .any(|block| block.language == DOT_WELL_LANGUAGE)
        })
    }

    #[must_use]
    pub fn article_animation_active(&self) -> bool {
        self.article_loading || self.article_has_animation()
    }

    #[must_use]
    pub const fn article_scroll(&self) -> u16 {
        self.article_scroll
    }

    pub fn begin_articles_load(&mut self) {
        self.articles_loading = true;
        self.articles_error = None;
    }

    pub fn begin_article_load(&mut self) {
        self.article_loading = true;
        self.articles_error = None;
    }

    pub fn set_opened_article(&mut self, article: ArticleContent) {
        if let Some(summary) = self
            .articles
            .iter_mut()
            .find(|summary| summary.slug == article.slug)
        {
            summary.labels.clone_from(&article.labels);
        }
        self.opened_article = Some(article);
        self.article_loading = false;
        self.articles_error = None;
        self.article_scroll = 0;
        self.article_scroll_limit = 0;
        self.python_running = false;
        self.python_output = None;
        self.article_animation_phase = 0;
        self.article_cursor = self
            .article_navigation()
            .map_or(0, |layout| layout.cursor_start);
        self.article_cursor_column = 0;
        self.article_preferred_column = 0;
    }

    pub fn set_articles_error(&mut self, error: impl Into<String>) {
        self.articles_loading = false;
        self.article_loading = false;
        self.articles_error = Some(error.into());
    }

    #[must_use]
    pub const fn hovered(&self) -> Option<HelpTarget> {
        self.hovered
    }

    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.should_quit
    }

    #[must_use]
    pub const fn awaiting_site_key(&self) -> bool {
        self.awaiting_site_key
    }

    #[must_use]
    pub const fn awaiting_article_g(&self) -> bool {
        self.awaiting_article_g
    }

    pub fn update(&mut self, message: Message) -> Option<Effect> {
        if !matches!(
            message,
            Message::BeginSiteShortcut | Message::CompleteSiteShortcut
        ) {
            self.awaiting_site_key = false;
        }
        if !matches!(message, Message::BeginArticleG | Message::CompleteArticleG) {
            self.awaiting_article_g = false;
        }

        match message {
            Message::NextTab => {
                let next = (self.selected.index() + 1) % Tab::ALL.len();
                self.selected = Tab::ALL[next];
            }
            Message::PreviousTab => {
                let previous = (self.selected.index() + Tab::ALL.len() - 1) % Tab::ALL.len();
                self.selected = Tab::ALL[previous];
            }
            Message::SelectTab(tab) => self.selected = tab,
            Message::SelectLanguage(language) => {
                self.language = language;
                self.show_language_notice();
            }
            Message::ToggleLanguage => {
                self.language = self.language.next();
                self.show_language_notice();
            }
            Message::SetOwnerOnline(online) => self.owner_online = online,
            Message::SetAuthenticated(authenticated) => self.authenticated = authenticated,
            Message::Hover(target) => self.hovered = target,
            Message::HideLanguageNotice(generation) => {
                if generation == self.language_notice_generation {
                    self.language_notice = false;
                }
            }
            Message::NextArticle => {
                if !self.articles.is_empty() {
                    self.selected_article = (self.selected_article + 1) % self.articles.len();
                }
            }
            Message::PreviousArticle => {
                if !self.articles.is_empty() {
                    self.selected_article =
                        (self.selected_article + self.articles.len() - 1) % self.articles.len();
                }
            }
            Message::SelectArticle(index) => {
                if index < self.articles.len() {
                    self.selected_article = index;
                }
            }
            Message::NextProject => {
                self.selected_project = (self.selected_project + 1) % PROJECTS.len();
            }
            Message::PreviousProject => {
                self.selected_project =
                    (self.selected_project + PROJECTS.len() - 1) % PROJECTS.len();
            }
            Message::SelectProject(index) => {
                if index < PROJECTS.len() {
                    self.selected_project = index;
                }
            }
            Message::OpenSelectedProject => {
                return Some(Effect::OpenUrl(self.selected_project().url));
            }
            Message::SelectArticleCursor(row) => self.select_article_cursor(row),
            Message::SelectArticlePosition { row, column } => {
                self.select_article_position(row, column);
            }
            Message::ScrollArticleDown => self.move_article_cursor(1),
            Message::ScrollArticleUp => self.move_article_cursor(-1),
            Message::MoveArticleCursorLeft => self.move_article_cursor_horizontal(-1),
            Message::MoveArticleCursorRight => self.move_article_cursor_horizontal(1),
            Message::MoveArticleCursorToLineStart => self.move_article_cursor_to_edge(false),
            Message::MoveArticleCursorToLineEnd => self.move_article_cursor_to_edge(true),
            Message::MoveArticleCursorToDocumentStart => {
                self.move_article_cursor_to_document(false)
            }
            Message::MoveArticleCursorToDocumentEnd => self.move_article_cursor_to_document(true),
            Message::DismissPythonOutput => {
                self.python_running = false;
                self.python_output = None;
                self.refresh_article_bounds();
            }
            Message::BeginArticleG => self.awaiting_article_g = true,
            Message::CompleteArticleG => {
                if self.awaiting_article_g {
                    self.move_article_cursor_to_document(false);
                }
                self.awaiting_article_g = false;
            }
            Message::CloseArticle => {
                self.opened_article = None;
                self.article_scroll = 0;
                self.article_scroll_limit = 0;
                self.article_cursor = 0;
                self.article_cursor_column = 0;
                self.article_preferred_column = 0;
                self.python_running = false;
                self.python_output = None;
                self.awaiting_article_g = false;
                self.article_animation_phase = 0;
            }
            Message::AdvanceSkeleton => {
                self.skeleton_phase = self.skeleton_phase.wrapping_add(1) % 24;
            }
            Message::AdvanceArticleAnimation => {
                if self.article_animation_active() {
                    self.article_animation_phase =
                        self.article_animation_phase.wrapping_add(1) % DOT_WELL_FRAMES;
                }
            }
            Message::BeginSiteShortcut => self.awaiting_site_key = true,
            Message::CompleteSiteShortcut => {
                let effect = self.awaiting_site_key.then_some(Effect::OpenUrl(SITE_URL));
                self.awaiting_site_key = false;
                return effect;
            }
            Message::CancelShortcut => {}
            Message::Quit => self.should_quit = true,
        }

        None
    }

    fn show_language_notice(&mut self) {
        self.language_notice = true;
        self.language_notice_generation = self.language_notice_generation.wrapping_add(1);
    }

    fn select_article_cursor(&mut self, row: u16) {
        let Some(layout) = self.article_navigation() else {
            return;
        };
        self.article_cursor = row.clamp(
            layout.cursor_start,
            layout.total_rows.saturating_sub(1).max(layout.cursor_start),
        );
        self.article_cursor_column = self.article_cursor_column.min(
            layout.row_widths[usize::from(self.article_cursor)]
                .max(1)
                .saturating_sub(1),
        );
        self.article_preferred_column = self.article_cursor_column;
        self.ensure_article_cursor_visible();
    }

    fn select_article_position(&mut self, row: u16, column: u16) {
        let Some(layout) = self.article_navigation() else {
            return;
        };
        self.article_cursor = row.clamp(
            layout.cursor_start,
            layout.total_rows.saturating_sub(1).max(layout.cursor_start),
        );
        self.article_cursor_column = column.min(
            layout.row_widths[usize::from(self.article_cursor)]
                .max(1)
                .saturating_sub(1),
        );
        self.article_preferred_column = self.article_cursor_column;
        self.ensure_article_cursor_visible();
    }

    fn move_article_cursor(&mut self, delta: i16) {
        let Some(layout) = self.article_navigation() else {
            return;
        };
        let end = layout.total_rows.saturating_sub(1).max(layout.cursor_start);
        self.article_cursor = self
            .article_cursor
            .saturating_add_signed(delta)
            .clamp(layout.cursor_start, end);
        self.article_cursor_column = self.article_preferred_column.min(
            layout.row_widths[usize::from(self.article_cursor)]
                .max(1)
                .saturating_sub(1),
        );
        self.ensure_article_cursor_visible();
    }

    fn move_article_cursor_horizontal(&mut self, delta: i16) {
        let width = self.article_line_width(self.article_cursor);
        self.article_cursor_column = self
            .article_cursor_column
            .saturating_add_signed(delta)
            .min(width.saturating_sub(1));
        self.article_preferred_column = self.article_cursor_column;
    }

    fn move_article_cursor_to_edge(&mut self, end: bool) {
        self.article_cursor_column = if end {
            self.article_line_width(self.article_cursor)
                .saturating_sub(1)
        } else {
            0
        };
        self.article_preferred_column = self.article_cursor_column;
    }

    fn move_article_cursor_to_document(&mut self, end: bool) {
        let Some(layout) = self.article_navigation() else {
            return;
        };
        self.article_cursor = if end {
            layout.total_rows.saturating_sub(1)
        } else {
            layout.cursor_start
        };
        self.article_cursor_column = 0;
        self.article_preferred_column = 0;
        self.ensure_article_cursor_visible();
    }

    fn ensure_article_cursor_visible(&mut self) {
        let viewport = self.article_viewport_rows.max(1);
        if self.article_cursor < self.article_scroll {
            self.article_scroll = self.article_cursor;
        } else if self.article_cursor >= self.article_scroll.saturating_add(viewport) {
            self.article_scroll = self
                .article_cursor
                .saturating_add(1)
                .saturating_sub(viewport);
        }
        self.article_scroll = self.article_scroll.min(self.article_scroll_limit);
    }

    fn refresh_article_bounds(&mut self) {
        let total_rows = self.article_total_rows();
        self.article_scroll_limit = total_rows.saturating_sub(self.article_viewport_rows.max(1));
        self.article_scroll = self.article_scroll.min(self.article_scroll_limit);
        if total_rows > 0 {
            self.article_cursor = self.article_cursor.min(total_rows - 1);
            self.article_cursor_column = self.article_cursor_column.min(
                self.article_line_width(self.article_cursor)
                    .saturating_sub(1),
            );
        }
    }

    fn article_navigation(&self) -> Option<ArticleNavigation> {
        let article = self.opened_article.as_ref()?;
        let raw_blocks = markdown_code_blocks(&article.markdown);
        let mut code_blocks = Vec::new();
        let mut rows = vec![
            "● SYNC  //  GITHUB main/articles".to_owned(),
            String::new(),
            article.title.clone(),
        ];
        if !article.labels.is_empty() {
            rows.push(
                article
                    .labels
                    .iter()
                    .take(3)
                    .fold(String::new(), |mut labels, label| {
                        labels.push(' ');
                        labels.push_str(label);
                        labels.push_str("  ");
                        labels
                    }),
            );
        }
        rows.push(String::new());
        let cursor_start = 2;
        let mut front_matter = false;
        let mut active_block = None::<(usize, u16, bool)>;
        let mut block_index = 0_usize;

        for (line_index, raw) in article.markdown.lines().enumerate() {
            if raw.trim() == "---" && (line_index == 0 || front_matter) {
                front_matter = !front_matter;
                continue;
            }
            if front_matter {
                continue;
            }
            if let Some(language) = fence_language(raw) {
                if let Some((index, start_row, animated)) = active_block.take() {
                    if !animated {
                        rows.push(format!("╰{}╯", "─".repeat(22)));
                    }
                    if let Some(block) = raw_blocks.get(index) {
                        code_blocks.push(ArticleCodeBlock {
                            index: block.index,
                            language: block.language.clone(),
                            code: block.code.clone(),
                            start_row,
                            end_row: rows.len().saturating_sub(1) as u16,
                        });
                    }
                    block_index += 1;
                } else {
                    let animated = language == DOT_WELL_LANGUAGE;
                    active_block = Some((block_index, rows.len() as u16, animated));
                    if animated {
                        rows.extend(std::iter::repeat_n(
                            " ".repeat(24),
                            usize::from(DOT_WELL_ROWS),
                        ));
                    } else {
                        let language = if language.is_empty() {
                            "TEXT".to_owned()
                        } else {
                            language.to_uppercase()
                        };
                        rows.push(padded_article_row(format!("╭─ {language}"), 24));
                    }
                }
                continue;
            }
            if active_block.is_some_and(|(_, _, animated)| !animated) {
                rows.push(format!("│ {raw}│"));
            } else if active_block.is_some() {
                continue;
            } else if let Some(source) = markdown_image_source(raw) {
                let (image_rows, image_width) = article
                    .images
                    .iter()
                    .find(|image| image.source == source)
                    .map_or((1, 1), |image| {
                        (image.height.div_ceil(2).max(1), image.width.max(1))
                    });
                rows.extend(std::iter::repeat_n(
                    " ".repeat(usize::from(image_width)),
                    usize::from(image_rows),
                ));
            } else {
                rows.push(markdown_display_text(raw));
            }
        }
        if let Some((index, start_row, animated)) = active_block
            && let Some(block) = raw_blocks.get(index)
        {
            if !animated {
                rows.push(format!("╰{}╯", "─".repeat(22)));
            }
            code_blocks.push(ArticleCodeBlock {
                index: block.index,
                language: block.language.clone(),
                code: block.code.clone(),
                start_row,
                end_row: rows.len().saturating_sub(1) as u16,
            });
        }

        rows.push(String::new());
        rows.push(" ".repeat(48));
        let total_rows = rows.len().min(usize::from(u16::MAX)) as u16;
        rows.truncate(usize::from(total_rows));
        let row_widths = rows
            .iter()
            .map(|row| text_width(row).max(1))
            .collect::<Vec<_>>();
        Some(ArticleNavigation {
            total_rows: total_rows.max(cursor_start + 1),
            cursor_start,
            code_blocks,
            row_widths,
        })
    }
}

fn padded_article_row(mut row: String, width: usize) -> String {
    let length = row.chars().count();
    if length < width {
        row.push_str(&" ".repeat(width - length));
    } else if length > width {
        row = row.chars().take(width).collect();
    }
    row
}

#[must_use]
pub fn markdown_code_blocks(markdown: &str) -> Vec<MarkdownCodeBlock> {
    let mut blocks = Vec::new();
    let mut current = None::<(String, String)>;
    for line in markdown.lines() {
        if let Some((language, code)) = &mut current {
            if fence_language(line).is_some() {
                blocks.push(MarkdownCodeBlock {
                    index: blocks.len(),
                    language: std::mem::take(language),
                    code: std::mem::take(code),
                });
                current = None;
            } else {
                if !code.is_empty() {
                    code.push('\n');
                }
                code.push_str(line);
            }
        } else if let Some(language) = fence_language(line) {
            current = Some((language, String::new()));
        }
    }
    if let Some((language, code)) = current {
        blocks.push(MarkdownCodeBlock {
            index: blocks.len(),
            language,
            code,
        });
    }
    blocks
}

#[must_use]
pub fn python_code_blocks(markdown: &str) -> Vec<String> {
    markdown_code_blocks(markdown)
        .into_iter()
        .filter(MarkdownCodeBlock::executable)
        .map(|block| block.code)
        .collect()
}

fn fence_language(line: &str) -> Option<String> {
    let fence = line.trim().strip_prefix("```")?;
    Some(
        fence
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_lowercase(),
    )
}

fn markdown_image_source(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("![")?;
    let (_, rest) = rest.split_once("](")?;
    rest.strip_suffix(')').map(str::trim)
}

fn text_width(text: &str) -> u16 {
    text.chars().count().min(usize::from(u16::MAX)) as u16
}

fn markdown_display_text(raw: &str) -> String {
    if let Some(text) = raw
        .strip_prefix("### ")
        .or_else(|| raw.strip_prefix("## "))
        .or_else(|| raw.strip_prefix("# "))
    {
        text.to_owned()
    } else if let Some(text) = raw.strip_prefix("- ") {
        format!("• {text}")
    } else if let Some(text) = raw.strip_prefix("> ") {
        format!("│ {text}")
    } else {
        raw.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        App, ArticleContent, ArticleSummary, DOT_WELL_ROWS, Effect, HelpTarget, Language, Message,
        PROJECTS, SITE_URL, Tab, markdown_code_blocks, python_code_blocks,
    };

    #[test]
    fn tab_navigation_wraps_in_both_directions() {
        let mut app = App::default();
        let _ = app.update(Message::PreviousTab);
        assert_eq!(app.selected(), Tab::Info);
        let _ = app.update(Message::NextTab);
        assert_eq!(app.selected(), Tab::Main);
    }

    #[test]
    fn language_presence_and_help_are_shared_state() {
        let mut app = App::default();
        let _ = app.update(Message::SelectLanguage(Language::Ru));
        let _ = app.update(Message::SetOwnerOnline(true));
        let _ = app.update(Message::SetAuthenticated(true));
        let _ = app.update(Message::Hover(Some(HelpTarget::Logo)));
        assert_eq!(app.language(), Language::Ru);
        assert!(app.owner_online());
        assert!(app.authenticated());
        assert_eq!(app.hovered(), Some(HelpTarget::Logo));
        assert!(app.language_notice());
        let generation = app.language_notice_generation();
        let _ = app.update(Message::HideLanguageNotice(generation));
        assert!(!app.language_notice());
    }

    #[test]
    fn gx_emits_the_open_site_effect() {
        let mut app = App::default();
        assert_eq!(app.update(Message::BeginSiteShortcut), None);
        assert_eq!(
            app.update(Message::CompleteSiteShortcut),
            Some(Effect::OpenUrl(SITE_URL))
        );
    }

    #[test]
    fn projects_have_wrapping_selection_and_open_effects() {
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Projects));
        assert_eq!(app.selected_project_index(), 0);
        let _ = app.update(Message::PreviousProject);
        assert_eq!(app.selected_project_index(), PROJECTS.len() - 1);
        assert_eq!(
            app.update(Message::OpenSelectedProject),
            Some(Effect::OpenUrl(PROJECTS[1].url))
        );
        let _ = app.update(Message::NextProject);
        assert_eq!(app.selected_project_index(), 0);
    }

    #[test]
    fn github_articles_have_loading_selection_and_open_states() {
        let mut app = App::default();
        app.begin_articles_load();
        assert!(app.articles_loading());
        let _ = app.update(Message::AdvanceSkeleton);
        assert_eq!(app.skeleton_phase(), 1);
        app.set_articles(vec![
            ArticleSummary {
                slug: "one".into(),
                title_en: "One".into(),
                title_ru: "Один".into(),
                published: true,
                source_path: Some("articles/one.md".into()),
                edit_url: Some("https://example.com/one".into()),
                labels: vec!["cryptography".into()],
            },
            ArticleSummary {
                slug: "two".into(),
                title_en: "Two".into(),
                title_ru: "Два".into(),
                published: true,
                source_path: Some("articles/two.md".into()),
                edit_url: Some("https://example.com/two".into()),
                labels: Vec::new(),
            },
        ]);
        let _ = app.update(Message::NextArticle);
        assert_eq!(
            app.selected_article().map(|item| item.slug.as_str()),
            Some("two")
        );
        app.begin_article_load();
        app.set_opened_article(ArticleContent {
            slug: "two".into(),
            title: "Two".into(),
            markdown: "# Two".into(),
            images: Vec::new(),
            labels: vec!["Python".into()],
        });
        assert!(app.opened_article().is_some());
        assert_eq!(app.selected_article().unwrap().labels, ["Python"]);
        app.set_article_viewport_rows(3);
        let _ = app.update(Message::ScrollArticleDown);
        assert_eq!(app.article_scroll(), 1);
        let _ = app.update(Message::CloseArticle);
        assert!(app.opened_article().is_none());
        assert_eq!(app.article_scroll(), 0);
    }

    #[test]
    fn python_fences_are_extracted_without_other_languages() {
        assert_eq!(
            python_code_blocks("```rust\nfn main() {}\n```\n```python\nprint(42)\n```"),
            vec!["print(42)"]
        );
    }

    #[test]
    fn markdown_blocks_keep_stable_indices_and_normalized_languages() {
        let blocks = markdown_code_blocks(
            "```rust\nfn main() {}\n```\n```PYTHON extra\nprint(42)\n```\n```\nplain",
        );
        assert_eq!(blocks.len(), 3);
        assert_eq!((blocks[0].index, blocks[0].language.as_str()), (0, "rust"));
        assert_eq!(
            (blocks[1].index, blocks[1].language.as_str()),
            (1, "python")
        );
        assert_eq!(blocks[1].code, "print(42)");
        assert_eq!((blocks[2].index, blocks[2].language.as_str()), (2, ""));
        assert_eq!(blocks[2].code, "plain");
    }

    #[test]
    fn article_cursor_focuses_one_block_and_stops_at_document_end() {
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.set_opened_article(ArticleContent {
            slug: "blocks".into(),
            title: "Blocks".into(),
            markdown: concat!(
                "Intro\n",
                "```rust\nlet value = 1;\n```\n",
                "Between\n",
                "```Python\nprint(42)\nprint(43)\n```\n",
                "End"
            )
            .into(),
            images: Vec::new(),
            labels: Vec::new(),
        });
        app.set_article_viewport_rows(4);

        let rust = app.article_code_block(0).unwrap();
        let python = app.article_code_block(1).unwrap();
        assert!(!rust.executable());
        assert!(python.executable());

        let _ = app.update(Message::SelectArticleCursor(rust.start_row));
        assert_eq!(app.focused_code_block().unwrap().index, 0);
        assert_eq!(app.python_code(), None);

        let _ = app.update(Message::SelectArticleCursor(python.end_row));
        assert_eq!(app.focused_code_block().unwrap().index, 1);
        assert_eq!(app.python_code().as_deref(), Some("print(42)\nprint(43)"));

        for _ in 0..100 {
            let _ = app.update(Message::ScrollArticleDown);
        }
        assert_eq!(app.article_cursor(), app.article_total_rows() - 1);
        assert_eq!(app.article_scroll(), app.article_scroll_limit());
    }

    #[test]
    fn dot_well_has_fixed_navigation_rows_and_an_advancing_phase() {
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.set_opened_article(ArticleContent {
            slug: "animation".into(),
            title: "Animation".into(),
            markdown: "Before\n```DOT-WELL\n```\nAfter".into(),
            images: Vec::new(),
            labels: vec!["animation".into()],
        });

        assert!(app.article_has_animation());
        let block = app.article_code_block(0).unwrap();
        assert!(block.animated());
        assert_eq!(block.end_row - block.start_row + 1, DOT_WELL_ROWS);
        let _ = app.update(Message::AdvanceArticleAnimation);
        assert_eq!(app.article_animation_phase(), 1);
        let _ = app.update(Message::CloseArticle);
        assert_eq!(app.article_animation_phase(), 0);
    }

    #[test]
    fn article_cursor_moves_by_line_and_keeps_its_column() {
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.set_opened_article(ArticleContent {
            slug: "reader".into(),
            title: "Reader".into(),
            markdown: "abcdefgh\nx\nabcdefgh".into(),
            images: Vec::new(),
            labels: Vec::new(),
        });
        app.set_article_viewport_rows(4);

        let _ = app.update(Message::SelectArticlePosition { row: 4, column: 6 });
        assert_eq!((app.article_cursor(), app.article_cursor_column()), (4, 6));
        let _ = app.update(Message::ScrollArticleDown);
        assert_eq!((app.article_cursor(), app.article_cursor_column()), (5, 0));
        let _ = app.update(Message::ScrollArticleDown);
        assert_eq!((app.article_cursor(), app.article_cursor_column()), (6, 6));

        let _ = app.update(Message::MoveArticleCursorLeft);
        assert_eq!(app.article_cursor_column(), 5);
        let _ = app.update(Message::MoveArticleCursorToLineStart);
        assert_eq!(app.article_cursor_column(), 0);
        let _ = app.update(Message::MoveArticleCursorToLineEnd);
        assert_eq!(app.article_cursor_column(), 7);
        let _ = app.update(Message::MoveArticleCursorRight);
        assert_eq!(app.article_cursor_column(), 7);
    }

    #[test]
    fn article_loading_advances_the_shared_animation() {
        let mut app = App::default();
        app.begin_article_load();
        assert!(app.article_animation_active());
        let _ = app.update(Message::AdvanceArticleAnimation);
        assert_eq!(app.article_animation_phase(), 1);
    }

    #[test]
    fn python_output_is_dismissed_without_closing_the_article() {
        let mut app = App::default();
        app.set_opened_article(ArticleContent {
            slug: "python".into(),
            title: "Python".into(),
            markdown: "```python\nprint(42)\n```".into(),
            images: Vec::new(),
            labels: Vec::new(),
        });
        app.finish_python_run("42");
        let _ = app.update(Message::DismissPythonOutput);
        assert_eq!(app.python_output(), None);
        assert!(!app.python_running());
        assert!(app.opened_article().is_some());
    }
}
