use std::{
    cell::{Cell, RefCell},
    io,
    rc::Rc,
};

use gloo_timers::future::TimeoutFuture;
use ratzilla::{
    DomBackend, WebRenderer,
    event::{KeyCode, MouseButton, MouseEventKind},
    ratatui::Terminal,
};
use svetsec_core::{
    App, ArticleContent, ArticleImage, ArticleSummary, Effect, Message, SITE_URL, Tab,
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Request, RequestCredentials, RequestInit, Response};

fn main() -> io::Result<()> {
    let app = Rc::new(RefCell::new(App::default()));
    let viewport = Rc::new(Cell::new(ratzilla::ratatui::layout::Rect::default()));
    let browser_image_count = Rc::new(Cell::new(0_usize));
    let backend = DomBackend::new_by_id("terminal")?;
    let mut terminal = Terminal::new(backend)?;

    terminal.on_key_event({
        let app = Rc::clone(&app);
        move |event| {
            if char_is(&event.code, &['a', 'ф']) {
                begin_login(Rc::clone(&app));
                return;
            }
            let selected = app.borrow().selected();
            if selected == Tab::Articles {
                if matches!(&event.code, KeyCode::Up) || char_is(&event.code, &['k', 'л']) {
                    let message = if app.borrow().opened_article().is_some() {
                        Message::ScrollArticleUp
                    } else {
                        Message::PreviousArticle
                    };
                    let _ = app.borrow_mut().update(message);
                    return;
                }
                if matches!(&event.code, KeyCode::Down) || char_is(&event.code, &['j', 'о']) {
                    let message = if app.borrow().opened_article().is_some() {
                        Message::ScrollArticleDown
                    } else {
                        Message::NextArticle
                    };
                    let _ = app.borrow_mut().update(message);
                    return;
                }
                if matches!(&event.code, KeyCode::Enter) || char_is(&event.code, &['o', 'щ']) {
                    load_selected_article(Rc::clone(&app));
                    return;
                }
                if matches!(&event.code, KeyCode::Esc) && app.borrow().opened_article().is_some() {
                    let _ = app.borrow_mut().update(Message::CloseArticle);
                    return;
                }
                if char_is(&event.code, &['e', 'у']) {
                    if app.borrow().authenticated() {
                        open_article_editor(&app, false);
                    } else {
                        begin_login(Rc::clone(&app));
                    }
                    return;
                }
                if char_is(&event.code, &['n', 'т']) {
                    if app.borrow().authenticated() {
                        open_article_editor(&app, true);
                    } else {
                        begin_login(Rc::clone(&app));
                    }
                    return;
                }
                if char_is(&event.code, &['f', 'а']) {
                    load_articles(Rc::clone(&app), true);
                    return;
                }
            }
            let was_articles = selected == Tab::Articles;
            let language_toggled = char_is(&event.code, &['r', 'к']);
            if let Some(effect) = app.borrow_mut().update(message_for_key(event.code)) {
                apply_effect(effect);
            }
            if !was_articles && app.borrow().selected() == Tab::Articles {
                load_articles(Rc::clone(&app), false);
            }
            if language_toggled {
                schedule_language_notice_hide(Rc::clone(&app));
            }
        }
    })?;

    terminal.on_mouse_event({
        let app = Rc::clone(&app);
        let viewport = Rc::clone(&viewport);
        move |event| {
            if event.kind == MouseEventKind::Exited {
                let _ = app.borrow_mut().update(Message::Hover(None));
                return;
            }

            let area = viewport.get();
            let target = svetsec_ui::help_target_at(area, event.col, event.row);
            let _ = app.borrow_mut().update(Message::Hover(target));

            if matches!(
                event.kind,
                MouseEventKind::ButtonDown(MouseButton::Left)
                    | MouseEventKind::SingleClick(MouseButton::Left)
            ) {
                let article = {
                    let app = app.borrow();
                    svetsec_ui::article_at(area, event.col, event.row, &app)
                };
                if let Some(index) = article {
                    let _ = app.borrow_mut().update(Message::SelectArticle(index));
                    load_selected_article(Rc::clone(&app));
                    return;
                }
                if let Some(tab) = svetsec_ui::tab_at(area, event.col, event.row) {
                    let _ = app.borrow_mut().update(Message::SelectTab(tab));
                    if tab == Tab::Articles {
                        load_articles(Rc::clone(&app), false);
                    }
                }
            }
        }
    })?;

    poll_session(Rc::clone(&app));
    animate_skeleton(Rc::clone(&app));

    terminal.draw_web(move |frame| {
        viewport.set(frame.area());
        let app = app.borrow();
        svetsec_ui::render(frame, &app);
        let _ = sync_browser_images(&app, frame.area(), &browser_image_count);
    });
    Ok(())
}

fn sync_browser_images(
    app: &App,
    area: ratzilla::ratatui::layout::Rect,
    previous_count: &Cell<usize>,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    let Some(first_row) = grid.query_selector("pre")? else {
        return Ok(());
    };
    let Some(first_cell) = first_row.query_selector("span")? else {
        return Ok(());
    };
    let Some(layer) = document.get_element_by_id("article-media-layer") else {
        return Ok(());
    };
    let row_rect = first_row.get_bounding_client_rect();
    let cell_rect = first_cell.get_bounding_client_rect();
    let cell_width = cell_rect.width();
    let row_height = row_rect.height();
    if cell_width <= 0.0 || row_height <= 0.0 {
        return Ok(());
    }

    let placements = svetsec_ui::article_image_placements(area, app);
    for (index, placement) in placements.iter().enumerate() {
        let id = format!("article-native-image-{index}");
        let image = match document.get_element_by_id(&id) {
            Some(image) => image,
            None => {
                let image = document.create_element("img")?;
                image.set_attribute("id", &id)?;
                image.set_attribute("class", "native-article-image")?;
                layer.append_child(&image)?;
                image
            }
        };
        let left = row_rect.left() + f64::from(placement.x) * cell_width;
        let top = row_rect.top() + f64::from(placement.y) * row_height;
        let width = f64::from(placement.width) * cell_width;
        let height = f64::from(placement.height) * row_height;
        let clip_top = f64::from(placement.clip_top) * row_height;
        let clip_right = f64::from(placement.clip_right) * cell_width;
        let clip_bottom = f64::from(placement.clip_bottom) * row_height;
        image.set_attribute("src", &format!("/api/github/assets/{}", placement.source))?;
        image.set_attribute("alt", placement.alt)?;
        image.set_attribute(
            "style",
            &format!(
                "display:block;left:{left}px;top:{top}px;width:{width}px;height:{height}px;\
                 clip-path:inset({clip_top}px {clip_right}px {clip_bottom}px 0px);"
            ),
        )?;
    }
    for index in placements.len()..previous_count.get() {
        if let Some(image) = document.get_element_by_id(&format!("article-native-image-{index}")) {
            image.set_attribute("style", "display:none")?;
        }
    }
    previous_count.set(placements.len());
    Ok(())
}

fn message_for_key(code: KeyCode) -> Message {
    match code {
        KeyCode::Right | KeyCode::Tab | KeyCode::Char('l' | 'д') => Message::NextTab,
        KeyCode::Left | KeyCode::Char('h' | 'р') => Message::PreviousTab,
        KeyCode::Char('1') => Message::SelectTab(Tab::Main),
        KeyCode::Char('2') => Message::SelectTab(Tab::Articles),
        KeyCode::Char('3') => Message::SelectTab(Tab::Info),
        KeyCode::Char('r' | 'к') => Message::ToggleLanguage,
        KeyCode::Char('g' | 'п') => Message::BeginSiteShortcut,
        KeyCode::Char('x' | 'ч') => Message::CompleteSiteShortcut,
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

fn poll_session(app: Rc<RefCell<App>>) {
    spawn_local(async move {
        loop {
            if let Ok(state) = fetch_session("POST", "/api/heartbeat", None).await {
                let _ = app
                    .borrow_mut()
                    .update(Message::SetAuthenticated(state.authenticated));
                let _ = app
                    .borrow_mut()
                    .update(Message::SetOwnerOnline(state.owner_online));
            }
            TimeoutFuture::new(15_000).await;
        }
    });
}

fn animate_skeleton(app: Rc<RefCell<App>>) {
    spawn_local(async move {
        loop {
            TimeoutFuture::new(100).await;
            if app.borrow().articles_loading() || app.borrow().article_loading() {
                let _ = app.borrow_mut().update(Message::AdvanceSkeleton);
            }
        }
    });
}

fn schedule_language_notice_hide(app: Rc<RefCell<App>>) {
    let generation = app.borrow().language_notice_generation();
    spawn_local(async move {
        TimeoutFuture::new(1_500).await;
        let _ = app
            .borrow_mut()
            .update(Message::HideLanguageNotice(generation));
    });
}

fn begin_login(app: Rc<RefCell<App>>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(password)) = window.prompt_with_message("Owner password") else {
        return;
    };
    spawn_local(async move {
        let body = serde_json::json!({ "password": password }).to_string();
        if let Ok(state) = fetch_session("POST", "/api/session", Some(body)).await {
            let _ = app
                .borrow_mut()
                .update(Message::SetAuthenticated(state.authenticated));
            let _ = app
                .borrow_mut()
                .update(Message::SetOwnerOnline(state.owner_online));
        }
    });
}

struct SessionState {
    authenticated: bool,
    owner_online: bool,
}

async fn fetch_session(
    method: &str,
    url: &str,
    body: Option<String>,
) -> Result<SessionState, JsValue> {
    let json = request_json(method, url, body).await?;
    let authenticated = js_sys::Reflect::get(&json, &JsValue::from_str("authenticated"))?
        .as_bool()
        .unwrap_or(false);
    let owner_online = js_sys::Reflect::get(&json, &JsValue::from_str("owner_online"))?
        .as_bool()
        .unwrap_or(false);
    Ok(SessionState {
        authenticated,
        owner_online,
    })
}

fn load_articles(app: Rc<RefCell<App>>, force: bool) {
    if (!force && app.borrow().articles_loaded()) || app.borrow().articles_loading() {
        return;
    }
    app.borrow_mut().begin_articles_load();
    spawn_local(async move {
        match fetch_articles(force).await {
            Ok((articles, create_url)) => {
                let mut app = app.borrow_mut();
                app.set_articles(articles);
                app.set_article_create_url(create_url);
            }
            Err(_) => {
                let error = match app.borrow().language() {
                    svetsec_core::Language::En => "Could not load articles from GitHub.",
                    svetsec_core::Language::Ru => "Не удалось загрузить статьи с GitHub.",
                };
                app.borrow_mut().set_articles_error(error);
            }
        }
    });
}

fn load_selected_article(app: Rc<RefCell<App>>) {
    if app.borrow().opened_article().is_some() || app.borrow().article_loading() {
        return;
    }
    let Some(slug) = app
        .borrow()
        .selected_article()
        .map(|article| article.slug.clone())
    else {
        return;
    };
    app.borrow_mut().begin_article_load();
    spawn_local(async move {
        match fetch_article(&slug).await {
            Ok(article) => app.borrow_mut().set_opened_article(article),
            Err(_) => {
                let error = match app.borrow().language() {
                    svetsec_core::Language::En => "Could not load this Markdown file.",
                    svetsec_core::Language::Ru => "Не удалось загрузить Markdown-файл.",
                };
                app.borrow_mut().set_articles_error(error);
            }
        }
    });
}

fn open_article_editor(app: &Rc<RefCell<App>>, create: bool) {
    let url = if create {
        app.borrow().article_create_url().map(str::to_owned)
    } else {
        app.borrow().article_editor_url().map(str::to_owned)
    };
    if let Some(url) = url {
        let _ = ratzilla::utils::open_url(&url, true);
    }
}

async fn fetch_articles(refresh: bool) -> Result<(Vec<ArticleSummary>, String), JsValue> {
    let url = if refresh {
        "/api/github/articles?refresh=1"
    } else {
        "/api/github/articles"
    };
    let json = request_json("GET", url, None).await?;
    let array = js_sys::Reflect::get(&json, &JsValue::from_str("articles"))?;
    let mut articles = Vec::new();
    for value in js_sys::Array::from(&array).iter() {
        let string = |field: &str| {
            js_sys::Reflect::get(&value, &JsValue::from_str(field))
                .ok()
                .and_then(|value| value.as_string())
                .unwrap_or_default()
        };
        articles.push(ArticleSummary {
            slug: string("slug"),
            title_en: string("title_en"),
            title_ru: string("title_ru"),
            published: js_sys::Reflect::get(&value, &JsValue::from_str("published"))?
                .as_bool()
                .unwrap_or(false),
            source_path: Some(string("source_path")),
            edit_url: Some(string("edit_url")),
        });
    }
    let create_url = js_sys::Reflect::get(&json, &JsValue::from_str("create_url"))?
        .as_string()
        .unwrap_or_default();
    Ok((articles, create_url))
}

async fn fetch_article(slug: &str) -> Result<ArticleContent, JsValue> {
    let json = request_json("GET", &format!("/api/github/articles/{slug}"), None).await?;
    let string = |field: &str| {
        js_sys::Reflect::get(&json, &JsValue::from_str(field))
            .ok()
            .and_then(|value| value.as_string())
            .unwrap_or_default()
    };
    let image_values = js_sys::Reflect::get(&json, &JsValue::from_str("images"))?;
    let mut images = Vec::new();
    for value in js_sys::Array::from(&image_values).iter() {
        let image_string = |field: &str| {
            js_sys::Reflect::get(&value, &JsValue::from_str(field))
                .ok()
                .and_then(|value| value.as_string())
                .unwrap_or_default()
        };
        let number = |field: &str| {
            js_sys::Reflect::get(&value, &JsValue::from_str(field))
                .ok()
                .and_then(|value| value.as_f64())
                .unwrap_or_default() as u16
        };
        let pixel_values = js_sys::Reflect::get(&value, &JsValue::from_str("pixels"))?;
        let pixels = js_sys::Array::from(&pixel_values)
            .iter()
            .map(|value| value.as_f64().unwrap_or_default() as u8)
            .collect();
        images.push(ArticleImage {
            source: image_string("source"),
            alt: image_string("alt"),
            width: number("width"),
            height: number("height"),
            pixels,
        });
    }
    Ok(ArticleContent {
        slug: string("slug"),
        title: string("title"),
        markdown: string("markdown"),
        images,
    })
}

fn char_is(code: &KeyCode, characters: &[char]) -> bool {
    matches!(code, KeyCode::Char(character) if characters.contains(character))
}

async fn request_json(method: &str, url: &str, body: Option<String>) -> Result<JsValue, JsValue> {
    let options = RequestInit::new();
    options.set_method(method);
    options.set_credentials(RequestCredentials::SameOrigin);
    if let Some(body) = body {
        options.set_body(&JsValue::from_str(&body));
    }
    let request = Request::new_with_str_and_init(url, &options)?;
    request.headers().set("Accept", "application/json")?;
    request.headers().set("Content-Type", "application/json")?;

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await?
        .dyn_into::<Response>()?;
    if !response.ok() {
        return Err(JsValue::from_str("session request failed"));
    }
    JsFuture::from(response.json()?).await
}
