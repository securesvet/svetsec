use std::{
    cell::{Cell, RefCell},
    io,
    rc::Rc,
};

use ratzilla::{
    DomBackend, WebRenderer,
    event::{KeyCode, MouseButton, MouseEventKind},
    ratatui::Terminal,
};
use svetsec_core::{App, Effect, Message, SITE_URL, Tab};

fn main() -> io::Result<()> {
    let app = Rc::new(RefCell::new(App::default()));
    let viewport = Rc::new(Cell::new(ratzilla::ratatui::layout::Rect::default()));
    let backend = DomBackend::new()?;
    let mut terminal = Terminal::new(backend)?;

    terminal.on_key_event({
        let app = Rc::clone(&app);
        move |event| {
            if let Some(effect) = app.borrow_mut().update(message_for_key(event.code)) {
                apply_effect(effect);
            }
        }
    })?;

    terminal.on_mouse_event({
        let app = Rc::clone(&app);
        let viewport = Rc::clone(&viewport);
        move |event| {
            if matches!(
                event.kind,
                MouseEventKind::ButtonDown(MouseButton::Left)
                    | MouseEventKind::SingleClick(MouseButton::Left)
            ) {
                if let Some(tab) = svetsec_ui::tab_at(viewport.get(), event.col, event.row) {
                    let _ = app.borrow_mut().update(Message::SelectTab(tab));
                }
            }
        }
    })?;

    terminal.draw_web(move |frame| {
        viewport.set(frame.area());
        svetsec_ui::render(frame, &app.borrow());
    });
    Ok(())
}

fn message_for_key(code: KeyCode) -> Message {
    match code {
        KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => Message::NextTab,
        KeyCode::Left | KeyCode::Char('h') => Message::PreviousTab,
        KeyCode::Char('1') => Message::SelectTab(Tab::Main),
        KeyCode::Char('2') => Message::SelectTab(Tab::Articles),
        KeyCode::Char('3') => Message::SelectTab(Tab::Info),
        KeyCode::Char('g') => Message::BeginSiteShortcut,
        KeyCode::Char('x') => Message::CompleteSiteShortcut,
        _ => Message::CancelShortcut,
    }
}

fn apply_effect(effect: Effect) {
    match effect {
        Effect::OpenSite => {
            let _ = ratzilla::utils::open_url(SITE_URL, true);
        }
    }
}
