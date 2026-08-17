use std::{io, panic};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{DefaultTerminal, init, restore};
use svetsec_core::{App, Effect, Message, SITE_URL, Tab};

fn main() -> io::Result<()> {
    install_panic_hook();
    let mut terminal = init();
    let result = run(&mut terminal);
    restore();
    result
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::default();

    while !app.should_quit() {
        terminal.draw(|frame| svetsec_ui::render(frame, &app))?;

        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            if let Some(effect) = app.update(message_for_key(key.code)) {
                apply_effect(effect)?;
            }
        }
    }

    Ok(())
}

fn message_for_key(code: KeyCode) -> Message {
    match code {
        KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => Message::NextTab,
        KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => Message::PreviousTab,
        KeyCode::Char('1') => Message::SelectTab(Tab::Main),
        KeyCode::Char('2') => Message::SelectTab(Tab::Articles),
        KeyCode::Char('3') => Message::SelectTab(Tab::Info),
        KeyCode::Char('g') => Message::BeginSiteShortcut,
        KeyCode::Char('x') => Message::CompleteSiteShortcut,
        KeyCode::Char('q') | KeyCode::Esc => Message::Quit,
        _ => Message::CancelShortcut,
    }
}

fn apply_effect(effect: Effect) -> io::Result<()> {
    match effect {
        Effect::OpenSite => open::that(SITE_URL).map_err(io::Error::other),
    }
}

fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        restore();
        original_hook(panic_info);
    }));
}
