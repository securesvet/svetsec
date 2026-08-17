pub const SITE_URL: &str = "https://svetsec.ru";

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
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Tab {
    #[default]
    Main,
    Articles,
    Info,
}

impl Tab {
    pub const ALL: [Self; 3] = [Self::Main, Self::Articles, Self::Info];

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Main => 0,
            Self::Articles => 1,
            Self::Info => 2,
        }
    }

    #[must_use]
    pub const fn label(self, language: Language) -> &'static str {
        match (self, language) {
            (Self::Main, Language::En) => "Main",
            (Self::Articles, Language::En) => "Articles",
            (Self::Info, Language::En) => "Info",
            (Self::Main, Language::Ru) => "Главная",
            (Self::Articles, Language::Ru) => "Статьи",
            (Self::Info, Language::Ru) => "О сайте",
        }
    }

    #[must_use]
    pub const fn title(self, language: Language) -> &'static str {
        match (self, language) {
            (Self::Main, Language::En) => "Hello, internet.",
            (Self::Articles, Language::En) => "Articles",
            (Self::Info, Language::En) => "About svetsec.ru",
            (Self::Main, Language::Ru) => "Привет, интернет.",
            (Self::Articles, Language::Ru) => "Статьи",
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
            (Self::Info, Language::En) => {
                "svetsec.ru is a personal site built as a cross-platform TUI experiment."
            }
            (Self::Main, Language::Ru) => {
                "Небольшой уголок интернета на одном Rust-коде для терминала и браузера."
            }
            (Self::Articles, Language::Ru) => {
                "Заметки о безопасности, Rust и системах. Владелец может нажать e для записи."
            }
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
    ScrollArticleDown,
    ScrollArticleUp,
    CloseArticle,
    AdvanceSkeleton,
    BeginSiteShortcut,
    CompleteSiteShortcut,
    CancelShortcut,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    OpenSite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArticleSummary {
    pub slug: String,
    pub title_en: String,
    pub title_ru: String,
    pub published: bool,
    pub source_path: Option<String>,
    pub edit_url: Option<String>,
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

#[derive(Debug, Default, Eq, PartialEq)]
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
    opened_article: Option<ArticleContent>,
    articles_error: Option<String>,
    article_create_url: Option<String>,
    skeleton_phase: u8,
    article_scroll: u16,
    language_notice: bool,
    language_notice_generation: u64,
    hovered: Option<HelpTarget>,
    awaiting_site_key: bool,
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
        self.opened_article = Some(article);
        self.article_loading = false;
        self.articles_error = None;
        self.article_scroll = 0;
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

    pub fn update(&mut self, message: Message) -> Option<Effect> {
        if !matches!(
            message,
            Message::BeginSiteShortcut | Message::CompleteSiteShortcut
        ) {
            self.awaiting_site_key = false;
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
            Message::ScrollArticleDown => {
                self.article_scroll = self.article_scroll.saturating_add(1);
            }
            Message::ScrollArticleUp => {
                self.article_scroll = self.article_scroll.saturating_sub(1);
            }
            Message::CloseArticle => {
                self.opened_article = None;
                self.article_scroll = 0;
            }
            Message::AdvanceSkeleton => {
                self.skeleton_phase = self.skeleton_phase.wrapping_add(1) % 24;
            }
            Message::BeginSiteShortcut => self.awaiting_site_key = true,
            Message::CompleteSiteShortcut => {
                let effect = self.awaiting_site_key.then_some(Effect::OpenSite);
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
}

#[cfg(test)]
mod tests {
    use super::{App, ArticleContent, ArticleSummary, Effect, HelpTarget, Language, Message, Tab};

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
            Some(Effect::OpenSite)
        );
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
            },
            ArticleSummary {
                slug: "two".into(),
                title_en: "Two".into(),
                title_ru: "Два".into(),
                published: true,
                source_path: Some("articles/two.md".into()),
                edit_url: Some("https://example.com/two".into()),
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
        });
        assert!(app.opened_article().is_some());
        let _ = app.update(Message::ScrollArticleDown);
        assert_eq!(app.article_scroll(), 1);
        let _ = app.update(Message::CloseArticle);
        assert!(app.opened_article().is_none());
        assert_eq!(app.article_scroll(), 0);
    }
}
