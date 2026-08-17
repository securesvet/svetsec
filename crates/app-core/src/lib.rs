pub const SITE_URL: &str = "https://svetsec.ru";

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
    pub const fn label(self) -> &'static str {
        match self {
            Self::Main => "Main",
            Self::Articles => "Articles",
            Self::Info => "Info",
        }
    }

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Main => "Hello, internet.",
            Self::Articles => "Articles",
            Self::Info => "About svetsec.ru",
        }
    }

    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Main => {
                "A small corner of the internet, now running from the same Rust code in your terminal and browser."
            }
            Self::Articles => "Notes about security, Rust, and systems will appear here.",
            Self::Info => "svetsec.ru is a personal site built as a cross-platform TUI experiment.",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Message {
    NextTab,
    PreviousTab,
    SelectTab(Tab),
    BeginSiteShortcut,
    CompleteSiteShortcut,
    CancelShortcut,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    OpenSite,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct App {
    selected: Tab,
    awaiting_site_key: bool,
    should_quit: bool,
}

impl App {
    #[must_use]
    pub const fn selected(&self) -> Tab {
        self.selected
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
}

#[cfg(test)]
mod tests {
    use super::{App, Effect, Message, Tab};

    #[test]
    fn tab_navigation_wraps_in_both_directions() {
        let mut app = App::default();

        let _ = app.update(Message::PreviousTab);
        assert_eq!(app.selected(), Tab::Info);

        let _ = app.update(Message::NextTab);
        assert_eq!(app.selected(), Tab::Main);
    }

    #[test]
    fn a_tab_can_be_selected_directly() {
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        assert_eq!(app.selected(), Tab::Articles);
    }

    #[test]
    fn gx_emits_the_open_site_effect() {
        let mut app = App::default();

        assert_eq!(app.update(Message::BeginSiteShortcut), None);
        assert!(app.awaiting_site_key());
        assert_eq!(
            app.update(Message::CompleteSiteShortcut),
            Some(Effect::OpenSite)
        );
        assert!(!app.awaiting_site_key());
    }

    #[test]
    fn another_command_cancels_the_g_prefix() {
        let mut app = App::default();

        let _ = app.update(Message::BeginSiteShortcut);
        let _ = app.update(Message::NextTab);

        assert_eq!(app.update(Message::CompleteSiteShortcut), None);
    }
}
