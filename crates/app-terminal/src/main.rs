use std::{
    io, panic,
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{DefaultTerminal, init, restore};
use svetsec_core::{App, Effect, Message, Tab};

fn main() -> io::Result<()> {
    install_panic_hook();
    let mut terminal = init();
    let result = run(&mut terminal);
    restore();
    result
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::default();
    let mut language_notice_deadline = None;

    while !app.should_quit() {
        if let Some((deadline, generation)) = language_notice_deadline
            && Instant::now() >= deadline
        {
            let _ = app.update(Message::HideLanguageNotice(generation));
            language_notice_deadline = None;
        }
        terminal.draw(|frame| svetsec_ui::render(frame, &app))?;

        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let message = if app.selected() == Tab::Projects {
                project_message_for_key(key.code).unwrap_or_else(|| message_for_key(key.code))
            } else {
                message_for_key(key.code)
            };
            if let Some(effect) = app.update(message) {
                apply_effect(effect)?;
            }
            if matches!(key.code, KeyCode::Char('r' | 'к')) {
                language_notice_deadline = Some((
                    Instant::now() + Duration::from_millis(1_500),
                    app.language_notice_generation(),
                ));
            }
        }
    }

    Ok(())
}

fn message_for_key(code: KeyCode) -> Message {
    match code {
        KeyCode::Right | KeyCode::Tab | KeyCode::Char('l' | 'д') => Message::NextTab,
        KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h' | 'р') => Message::PreviousTab,
        KeyCode::Char('1') => Message::SelectTab(Tab::Main),
        KeyCode::Char('2') => Message::SelectTab(Tab::Articles),
        KeyCode::Char('3') => Message::SelectTab(Tab::Projects),
        KeyCode::Char('4') => Message::SelectTab(Tab::Info),
        KeyCode::Char('r' | 'к') => Message::ToggleLanguage,
        KeyCode::Char('g' | 'п') => Message::BeginSiteShortcut,
        KeyCode::Char('x' | 'ч') => Message::CompleteSiteShortcut,
        KeyCode::Char('q' | 'й') | KeyCode::Esc => Message::Quit,
        _ => Message::CancelShortcut,
    }
}

fn project_message_for_key(code: KeyCode) -> Option<Message> {
    match code {
        KeyCode::Up | KeyCode::Char('k' | 'л') => Some(Message::PreviousProject),
        KeyCode::Down | KeyCode::Char('j' | 'о') => Some(Message::NextProject),
        KeyCode::Enter | KeyCode::Char('o' | 'щ') => Some(Message::OpenSelectedProject),
        _ => None,
    }
}

fn apply_effect(effect: Effect) -> io::Result<()> {
    match effect {
        Effect::OpenUrl(url) => open::that(url).map_err(io::Error::other),
    }
}

fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        restore();
        original_hook(panic_info);
    }));
}
