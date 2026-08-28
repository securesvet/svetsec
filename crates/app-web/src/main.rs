use std::{
    cell::{Cell, RefCell},
    io,
    rc::Rc,
};

use gloo_timers::future::TimeoutFuture;
use ratzilla::{DomBackend, WebRenderer, event::KeyCode, ratatui::Terminal};
use svetsec_core::{
    App, ArticleContent, ArticleImage, ArticleSummary, Effect, HelpTarget, Message, Tab,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{KeyboardEvent, MouseEvent, Request, RequestCredentials, RequestInit, Response};

#[derive(Clone, Debug, Eq, PartialEq)]
struct DomSignature {
    area: ratzilla::ratatui::layout::Rect,
    selected: Tab,
    language: svetsec_core::Language,
    hovered: Option<HelpTarget>,
    articles_len: usize,
    articles_loading: bool,
    article_loading: bool,
    selected_project: usize,
    opened_slug: Option<String>,
    article_scroll: u16,
    article_cursor: u16,
    article_cursor_column: u16,
    python_running: bool,
    python_output: bool,
    language_notice: bool,
    image_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum WebRoute {
    Main,
    Articles,
    Article(String),
    Projects,
    Info,
}

impl WebRoute {
    fn from_path(path: &str) -> Self {
        let segments = path
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        match segments.as_slice() {
            [] | ["main"] => Self::Main,
            ["articles"] => Self::Articles,
            ["articles", slug]
                if !slug.is_empty()
                    && slug.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                    }) =>
            {
                Self::Article((*slug).to_owned())
            }
            ["info"] => Self::Info,
            ["projects"] => Self::Projects,
            _ => Self::Main,
        }
    }

    fn for_app(app: &App) -> Self {
        match app.selected() {
            Tab::Main => Self::Main,
            Tab::Info => Self::Info,
            Tab::Projects => Self::Projects,
            Tab::Articles => app.opened_article().map_or(Self::Articles, |article| {
                Self::Article(article.slug.clone())
            }),
        }
    }

    fn path(&self) -> String {
        match self {
            Self::Main => "/".into(),
            Self::Articles => "/articles".into(),
            Self::Article(slug) => format!("/articles/{slug}"),
            Self::Projects => "/projects".into(),
            Self::Info => "/info".into(),
        }
    }

    const fn tab(&self) -> Tab {
        match self {
            Self::Main => Tab::Main,
            Self::Articles | Self::Article(_) => Tab::Articles,
            Self::Projects => Tab::Projects,
            Self::Info => Tab::Info,
        }
    }
}

#[derive(Debug)]
struct RouteState {
    current: WebRoute,
    resolving: bool,
}

impl DomSignature {
    fn new(area: ratzilla::ratatui::layout::Rect, app: &App) -> Self {
        Self {
            area,
            selected: app.selected(),
            language: app.language(),
            hovered: app.hovered(),
            articles_len: app.articles().len(),
            articles_loading: app.articles_loading(),
            article_loading: app.article_loading(),
            selected_project: app.selected_project_index(),
            opened_slug: app.opened_article().map(|article| article.slug.clone()),
            article_scroll: app.article_scroll(),
            article_cursor: app.article_cursor(),
            article_cursor_column: app.article_cursor_column(),
            python_running: app.python_running(),
            python_output: app.python_output().is_some(),
            language_notice: app.language_notice(),
            image_count: app
                .opened_article()
                .map_or(usize::from(app.profile_image().is_some()), |article| {
                    article.images.len()
                }),
        }
    }
}

fn sync_browser_route(app: &App, route_state: &Rc<RefCell<RouteState>>) {
    let desired = WebRoute::for_app(app);
    let mut state = route_state.borrow_mut();
    if state.resolving || state.current == desired {
        return;
    }
    if let Some(window) = web_sys::window() {
        let _ = window.history().and_then(|history| {
            history.push_state_with_url(&JsValue::NULL, "", Some(&desired.path()))
        });
        if let Some(document) = window.document() {
            let title = match &desired {
                WebRoute::Main => "svetsec.ru".into(),
                WebRoute::Articles => "Articles — svetsec.ru".into(),
                WebRoute::Article(slug) => format!("{slug} — svetsec.ru"),
                WebRoute::Projects => "Projects — svetsec.ru".into(),
                WebRoute::Info => "Info — svetsec.ru".into(),
            };
            document.set_title(&title);
        }
    }
    state.current = desired;
}

fn install_route_events(
    app: Rc<RefCell<App>>,
    route_state: Rc<RefCell<RouteState>>,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let route_window = window.clone();
    let popstate = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        let route = route_window
            .location()
            .pathname()
            .ok()
            .map_or(WebRoute::Main, |path| WebRoute::from_path(&path));
        apply_web_route(Rc::clone(&app), route, Rc::clone(&route_state));
    });
    window.add_event_listener_with_callback("popstate", popstate.as_ref().unchecked_ref())?;
    popstate.forget();
    Ok(())
}

fn apply_web_route(app: Rc<RefCell<App>>, route: WebRoute, route_state: Rc<RefCell<RouteState>>) {
    {
        let mut state = route_state.borrow_mut();
        state.current = route.clone();
        state.resolving = matches!(route, WebRoute::Article(_));
    }
    let _ = app.borrow_mut().update(Message::SelectTab(route.tab()));
    match route {
        WebRoute::Main | WebRoute::Projects | WebRoute::Info => {
            if app.borrow().opened_article().is_some() {
                let _ = app.borrow_mut().update(Message::CloseArticle);
            }
            route_state.borrow_mut().resolving = false;
        }
        WebRoute::Articles => {
            if app.borrow().opened_article().is_some() {
                let _ = app.borrow_mut().update(Message::CloseArticle);
            }
            route_state.borrow_mut().resolving = false;
            load_articles(app, false);
        }
        WebRoute::Article(slug) => {
            if app
                .borrow()
                .opened_article()
                .is_some_and(|article| article.slug == slug)
            {
                route_state.borrow_mut().resolving = false;
                return;
            }
            let language = app.borrow().language();
            if !app.borrow().articles_loaded() {
                app.borrow_mut().begin_articles_load();
            }
            app.borrow_mut().begin_article_load();
            spawn_local(async move {
                if !app.borrow().articles_loaded() {
                    match fetch_articles(false, language).await {
                        Ok((articles, create_url)) => {
                            let mut app = app.borrow_mut();
                            app.set_articles(articles);
                            app.set_article_create_url(create_url);
                        }
                        Err(_) => {
                            app.borrow_mut()
                                .set_articles_error("Could not load articles.");
                            route_state.borrow_mut().resolving = false;
                            return;
                        }
                    }
                }
                let index = app
                    .borrow()
                    .articles()
                    .iter()
                    .position(|article| article.slug == slug);
                let Some(index) = index else {
                    app.borrow_mut().set_articles_error("Article not found.");
                    route_state.borrow_mut().resolving = false;
                    return;
                };
                let _ = app.borrow_mut().update(Message::SelectArticle(index));
                match fetch_article(&slug, language).await {
                    Ok(article) => app.borrow_mut().set_opened_article(article),
                    Err(_) => app
                        .borrow_mut()
                        .set_articles_error("Could not load this Markdown file."),
                }
                route_state.borrow_mut().resolving = false;
            });
        }
    }
}

fn main() -> io::Result<()> {
    let initial_route = web_sys::window()
        .and_then(|window| window.location().pathname().ok())
        .map_or(WebRoute::Main, |path| WebRoute::from_path(&path));
    let mut initial_app = App::default();
    if let Some(language) = stored_language() {
        initial_app.restore_language(language);
    }
    let _ = initial_app.update(Message::SelectTab(initial_route.tab()));
    initial_app.set_profile_image(ArticleImage {
        source: "/assets/profile.jpg".into(),
        alt: "Sviatoslav M.".into(),
        width: 18,
        height: 18,
        pixels: Vec::new(),
    });
    let app = Rc::new(RefCell::new(initial_app));
    let viewport = Rc::new(Cell::new(ratzilla::ratatui::layout::Rect::default()));
    let browser_image_count = Rc::new(Cell::new(0_usize));
    let dom_signature = Rc::new(RefCell::new(None::<DomSignature>));
    let route_state = Rc::new(RefCell::new(RouteState {
        current: initial_route.clone(),
        resolving: matches!(initial_route, WebRoute::Article(_)),
    }));
    let backend = DomBackend::new_by_id("terminal")?;
    let terminal = Terminal::new(backend)?;
    install_browser_events(Rc::clone(&app), Rc::clone(&viewport))
        .map_err(|error| io::Error::other(format!("browser event setup failed: {error:?}")))?;
    install_route_events(Rc::clone(&app), Rc::clone(&route_state))
        .map_err(|error| io::Error::other(format!("browser route setup failed: {error:?}")))?;

    poll_session(Rc::clone(&app));
    animate_ui(Rc::clone(&app));
    apply_web_route(Rc::clone(&app), initial_route, Rc::clone(&route_state));

    terminal.draw_web(move |frame| {
        viewport.set(frame.area());
        let app_handle = Rc::clone(&app);
        let mut app = app.borrow_mut();
        app.set_article_viewport_rows(svetsec_ui::article_viewport_rows(frame.area()));
        svetsec_ui::render(frame, &app);
        let signature = DomSignature::new(frame.area(), &app);
        let changed = dom_signature.borrow().as_ref() != Some(&signature);
        sync_browser_route(&app, &route_state);
        if changed {
            *dom_signature.borrow_mut() = Some(signature);
            let sync_app = Rc::clone(&app_handle);
            let sync_image_count = Rc::clone(&browser_image_count);
            let area = frame.area();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                let app = sync_app.borrow();
                let _ = sync_browser_tabs(area, &app);
                let _ = sync_browser_articles(area, &app);
                let _ = sync_browser_projects(area, &app);
                let _ = sync_browser_navigation_links(area, &app);
                let _ = sync_browser_code_actions(area, &app);
                let _ = sync_browser_output_close(area, &app);
                let _ = sync_browser_images(&app, area, &sync_image_count);
                let _ = sync_mobile_controls(&app);
                let _ = sync_browser_native_scroll(&app);
                let _ = sync_browser_text_selection();
            });
        }
    });
    Ok(())
}

fn sync_browser_tabs(area: ratzilla::ratatui::layout::Rect, app: &App) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    for (kind, tab) in svetsec_ui::tab_areas(area) {
        let selected = if app.selected() == kind {
            " web-tab-selected"
        } else {
            " web-tab-idle"
        };
        let hovered = if app.hovered() == Some(svetsec_core::HelpTarget::Tab(kind)) {
            " web-tab-hovered"
        } else {
            ""
        };
        for row in tab.top()..tab.bottom() {
            for column in tab.left()..tab.right() {
                let selector = format!("pre:nth-child({}) span:nth-child({})", row + 1, column + 1);
                let Some(cell) = grid.query_selector(&selector)? else {
                    continue;
                };
                let edge = if column == tab.left() {
                    " web-tab-start"
                } else if column + 1 == tab.right() {
                    " web-tab-end"
                } else {
                    ""
                };
                cell.set_attribute("class", &format!("web-tab-cell{edge}{selected}{hovered}"))?;
            }
        }
    }
    Ok(())
}

fn sync_browser_code_actions(
    area: ratzilla::ratatui::layout::Rect,
    app: &App,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    let stale_actions = grid.query_selector_all(".web-code-action")?;
    for index in 0..stale_actions.length() {
        if let Some(node) = stale_actions.item(index)
            && let Some(element) = node.dyn_ref::<web_sys::Element>()
        {
            element.remove_attribute("class")?;
        }
    }
    for (action, area) in svetsec_ui::code_action_areas(area, app) {
        let action_class = match action {
            svetsec_ui::CodeBlockAction::Run { .. } => "web-code-run",
            svetsec_ui::CodeBlockAction::Copy { .. } => "web-code-copy",
        };
        for column in area.left()..area.right() {
            let selector = format!(
                "pre:nth-child({}) span:nth-child({})",
                area.top() + 1,
                column + 1
            );
            let Some(cell) = grid.query_selector(&selector)? else {
                continue;
            };
            let edge = if column == area.left() {
                " web-code-start"
            } else if column + 1 == area.right() {
                " web-code-end"
            } else {
                ""
            };
            cell.set_attribute("class", &format!("web-code-action {action_class}{edge}"))?;
        }
    }
    Ok(())
}

fn sync_browser_articles(area: ratzilla::ratatui::layout::Rect, app: &App) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    clear_cell_class(&grid, ".web-article-link")?;
    for (_, area) in svetsec_ui::article_areas(area, app) {
        set_area_class(&grid, area, "web-clickable web-article-link")?;
    }
    Ok(())
}

fn sync_browser_projects(area: ratzilla::ratatui::layout::Rect, app: &App) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    clear_cell_class(&grid, ".web-project-card")?;
    for (_, area) in svetsec_ui::project_areas(area, app) {
        set_area_class(&grid, area, "web-clickable web-project-card")?;
    }
    Ok(())
}

fn sync_browser_navigation_links(
    area: ratzilla::ratatui::layout::Rect,
    app: &App,
) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    clear_cell_class(&grid, ".web-back-link")?;
    clear_cell_class(&grid, ".web-resume-link")?;
    if let Some(area) = svetsec_ui::article_back_area(area, app) {
        set_area_class(&grid, area, "web-clickable web-back-link")?;
    }
    if let Some(area) = svetsec_ui::resume_link_area(area, app) {
        set_area_class(&grid, area, "web-clickable web-resume-link")?;
    }
    Ok(())
}

fn sync_mobile_controls(app: &App) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(controls) = document.get_element_by_id("mobile-controls") else {
        return Ok(());
    };
    let article_open = app.selected() == Tab::Articles && app.opened_article().is_some();
    controls.set_attribute(
        "data-article-open",
        if article_open { "true" } else { "false" },
    )?;
    if let Some(body) = document.query_selector("body")? {
        body.set_attribute(
            "data-mobile-article-open",
            if article_open { "true" } else { "false" },
        )?;
    }
    if let Some(back) = controls.query_selector("[data-article-action=\"back\"]")? {
        back.set_text_content(Some(match app.language() {
            svetsec_core::Language::En => "← Articles",
            svetsec_core::Language::Ru => "← Статьи",
        }));
    }
    for (tab, selector) in [
        (Tab::Main, "main"),
        (Tab::Articles, "articles"),
        (Tab::Projects, "projects"),
        (Tab::Info, "info"),
    ] {
        let Some(link) = controls.query_selector(&format!("[data-mobile-tab=\"{selector}\"]"))?
        else {
            continue;
        };
        link.set_text_content(Some(tab.label(app.language())));
        if app.selected() == tab {
            link.set_attribute("aria-current", "page")?;
        } else {
            link.remove_attribute("aria-current")?;
        }
    }
    Ok(())
}

fn sync_browser_native_scroll(app: &App) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(terminal) = document.get_element_by_id("terminal") else {
        return Ok(());
    };
    let article_open = app.selected() == Tab::Articles && app.opened_article().is_some();
    if !article_open {
        terminal.remove_attribute("data-native-scroll")?;
        terminal.set_scroll_top(0);
        if let Some(spacer) = document.get_element_by_id("web-article-scroll-spacer") {
            spacer.remove();
        }
        return Ok(());
    }

    terminal.set_attribute("data-native-scroll", "true")?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    let Some(first_row) = grid.query_selector("pre")? else {
        return Ok(());
    };
    let row_height = first_row.get_bounding_client_rect().height();
    if row_height <= 0.0 {
        return Ok(());
    }

    let spacer = match document.get_element_by_id("web-article-scroll-spacer") {
        Some(spacer) => spacer,
        None => {
            let spacer = document.create_element("div")?;
            spacer.set_attribute("id", "web-article-scroll-spacer")?;
            terminal.append_child(&spacer)?;
            spacer
        }
    };
    spacer.set_attribute(
        "style",
        &format!(
            "height:{}px",
            f64::from(app.article_scroll_limit()) * row_height
        ),
    )?;

    let desired = f64::from(app.article_scroll()) * row_height;
    if (f64::from(terminal.scroll_top()) - desired).abs() >= 1.0 {
        terminal.set_scroll_top(desired.round() as i32);
    }
    Ok(())
}

fn sync_browser_output_close(
    area: ratzilla::ratatui::layout::Rect,
    app: &App,
) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    clear_cell_class(&grid, ".web-output-close")?;
    if let Some(area) = svetsec_ui::python_output_close_area(area, app) {
        set_area_class(&grid, area, "web-clickable web-output-close")?;
    }
    Ok(())
}

fn clear_cell_class(grid: &web_sys::Element, selector: &str) -> Result<(), JsValue> {
    let cells = grid.query_selector_all(selector)?;
    for index in 0..cells.length() {
        if let Some(node) = cells.item(index)
            && let Some(element) = node.dyn_ref::<web_sys::Element>()
        {
            element.remove_attribute("class")?;
        }
    }
    Ok(())
}

fn set_area_class(
    grid: &web_sys::Element,
    area: ratzilla::ratatui::layout::Rect,
    class_name: &str,
) -> Result<(), JsValue> {
    for row in area.top()..area.bottom() {
        for column in area.left()..area.right() {
            let selector = format!("pre:nth-child({}) span:nth-child({})", row + 1, column + 1);
            if let Some(cell) = grid.query_selector(&selector)? {
                cell.set_attribute("class", class_name)?;
            }
        }
    }
    Ok(())
}

fn sync_browser_text_selection() -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    if grid.has_attribute("data-block-selection") {
        return Ok(());
    }
    clear_cell_class(&grid, ".web-selectable-text")?;
    let stale_blocks = grid.query_selector_all("[data-text-block]")?;
    for index in 0..stale_blocks.length() {
        if let Some(node) = stale_blocks.item(index)
            && let Some(element) = node.dyn_ref::<web_sys::Element>()
        {
            element.remove_attribute("data-text-block")?;
        }
    }
    let rows = grid.query_selector_all("pre")?;
    let mut next_block = 0_u32;
    let mut previous_runs = Vec::<(usize, usize, u32)>::new();
    for row_index in 0..rows.length() {
        let Some(row) = rows.item(row_index) else {
            continue;
        };
        let Some(row) = row.dyn_ref::<web_sys::Element>() else {
            continue;
        };
        let cells = row.query_selector_all("span")?;
        let mut text = Vec::with_capacity(cells.length() as usize);
        for index in 0..cells.length() {
            text.push(
                cells
                    .item(index)
                    .and_then(|cell| cell.text_content())
                    .unwrap_or_default(),
            );
        }
        let runs = selection_runs(&text);
        if runs.is_empty() {
            previous_runs.clear();
            continue;
        }
        let mut current_runs = Vec::with_capacity(runs.len());
        for (start, end) in runs {
            let block = previous_runs
                .iter()
                .find(|(previous_start, previous_end, _)| {
                    start <= previous_end.saturating_add(2)
                        && *previous_start <= end.saturating_add(2)
                })
                .map_or_else(
                    || {
                        let block = next_block;
                        next_block = next_block.saturating_add(1);
                        block
                    },
                    |(_, _, block)| *block,
                );
            for index in start..=end {
                let Some(cell) = cells.item(index as u32) else {
                    continue;
                };
                let Some(cell) = cell.dyn_ref::<web_sys::Element>() else {
                    continue;
                };
                if cell
                    .get_attribute("class")
                    .is_some_and(|class| class.contains("web-"))
                {
                    continue;
                }
                cell.set_attribute("class", "web-selectable-text")?;
                cell.set_attribute("data-text-block", &block.to_string())?;
            }
            current_runs.push((start, end, block));
        }
        previous_runs = current_runs;
    }
    Ok(())
}

fn selection_runs(cells: &[String]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    let mut last_text = 0;
    let mut soft_gap = 0;
    for (index, cell) in cells.iter().enumerate() {
        if cell_contains_selectable_text(cell) {
            if start.is_some() && soft_gap >= 3 {
                runs.push((start.unwrap_or(index), last_text));
                start = None;
            }
            start.get_or_insert(index);
            last_text = index;
            soft_gap = 0;
        } else if start.is_some() && cell.chars().all(char::is_whitespace) {
            soft_gap += 1;
        } else if let Some(start) = start.take() {
            runs.push((start, last_text));
            soft_gap = 0;
        }
    }
    if let Some(start) = start {
        runs.push((start, last_text));
    }
    runs
}

fn cell_contains_selectable_text(text: &str) -> bool {
    text.chars().any(|character| {
        !character.is_whitespace()
            && !(('\u{2500}'..='\u{257f}').contains(&character))
            && !(('\u{2800}'..='\u{28ff}').contains(&character))
    })
}

fn activate_browser_text_block(grid: &web_sys::Element, block: &str) -> Result<(), JsValue> {
    grid.set_attribute("data-block-selection", "true")?;
    let cells = grid.query_selector_all(".web-selectable-text")?;
    for index in 0..cells.length() {
        let Some(node) = cells.item(index) else {
            continue;
        };
        let Some(cell) = node.dyn_ref::<web_sys::Element>() else {
            continue;
        };
        let class = if cell.get_attribute("data-text-block").as_deref() == Some(block) {
            "web-selectable-text web-selection-active"
        } else {
            "web-selectable-text web-selection-muted"
        };
        cell.set_attribute("class", class)?;
    }
    Ok(())
}

fn release_browser_text_block(grid: &web_sys::Element) -> Result<(), JsValue> {
    grid.remove_attribute("data-block-selection")?;
    let cells = grid.query_selector_all(".web-selection-active, .web-selection-muted")?;
    for index in 0..cells.length() {
        let Some(node) = cells.item(index) else {
            continue;
        };
        let Some(cell) = node.dyn_ref::<web_sys::Element>() else {
            continue;
        };
        cell.set_attribute("class", "web-selectable-text")?;
    }
    Ok(())
}

fn browser_has_text_selection() -> bool {
    web_sys::window()
        .and_then(|window| window.get_selection().ok().flatten())
        .is_some_and(|selection| !selection.is_collapsed())
}

fn install_browser_events(
    app: Rc<RefCell<App>>,
    viewport: Rc<Cell<ratzilla::ratatui::layout::Rect>>,
) -> Result<(), JsValue> {
    // RatZilla replaces its inner grid after a resize, so events must live on
    // the stable window/terminal nodes instead of that transient grid.
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let terminal = document
        .get_element_by_id("terminal")
        .ok_or_else(|| JsValue::from_str("terminal unavailable"))?;

    let scroll_app = Rc::clone(&app);
    let scroll_terminal = terminal.clone();
    let scroll = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        if scroll_app.borrow().selected() != Tab::Articles
            || scroll_app.borrow().opened_article().is_none()
        {
            return;
        }
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
            return;
        };
        let Ok(Some(first_row)) = grid.query_selector("pre") else {
            return;
        };
        let row_height = first_row.get_bounding_client_rect().height();
        if row_height <= 0.0 {
            return;
        }
        let row = (f64::from(scroll_terminal.scroll_top().max(0)) / row_height).round() as u16;
        let _ = scroll_app
            .borrow_mut()
            .update(Message::SetArticleScroll(row));
    });
    terminal.add_event_listener_with_callback("scroll", scroll.as_ref().unchecked_ref())?;
    scroll.forget();

    let selection_terminal = terminal.clone();
    let selection_start = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
        if event.button() != 0 {
            return;
        }
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let Some(block) = target.get_attribute("data-text-block") else {
            return;
        };
        let Some(grid) = selection_terminal
            .query_selector("#terminal_ratzilla_grid")
            .ok()
            .flatten()
        else {
            return;
        };
        let _ = activate_browser_text_block(&grid, &block);
    });
    terminal
        .add_event_listener_with_callback("mousedown", selection_start.as_ref().unchecked_ref())?;
    selection_start.forget();

    let selection_end = Closure::<dyn FnMut(MouseEvent)>::new(move |_: MouseEvent| {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
            return;
        };
        let _ = release_browser_text_block(&grid);
    });
    window.add_event_listener_with_callback("mouseup", selection_end.as_ref().unchecked_ref())?;
    selection_end.forget();

    if let Some(controls) = document.get_element_by_id("mobile-controls") {
        for (action, message) in [
            ("back", Message::CloseArticle),
            ("up", Message::ScrollArticleUp),
            ("down", Message::ScrollArticleDown),
        ] {
            let Some(button) =
                controls.query_selector(&format!("[data-article-action=\"{action}\"]"))?
            else {
                continue;
            };
            let button_app = Rc::clone(&app);
            let activate = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
                event.prevent_default();
                let _ = button_app.borrow_mut().update(message);
            });
            button.add_event_listener_with_callback("click", activate.as_ref().unchecked_ref())?;
            activate.forget();
        }

        for (selector, tab) in [
            ("main", Tab::Main),
            ("articles", Tab::Articles),
            ("projects", Tab::Projects),
            ("info", Tab::Info),
        ] {
            let Some(link) =
                controls.query_selector(&format!("[data-mobile-tab=\"{selector}\"]"))?
            else {
                continue;
            };
            let tab_app = Rc::clone(&app);
            let activate = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
                event.prevent_default();
                if tab_app.borrow().opened_article().is_some() {
                    let _ = tab_app.borrow_mut().update(Message::CloseArticle);
                }
                let _ = tab_app.borrow_mut().update(Message::SelectTab(tab));
                if tab == Tab::Articles {
                    load_articles(Rc::clone(&tab_app), false);
                }
            });
            link.add_event_listener_with_callback("click", activate.as_ref().unchecked_ref())?;
            activate.forget();
        }
    }

    let key_app = Rc::clone(&app);
    let keydown = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        if event.meta_key() || event.ctrl_key() || event.alt_key() {
            return;
        }
        if let Some(code) = browser_key_code(&event.key()) {
            event.prevent_default();
            handle_key(Rc::clone(&key_app), code);
        }
    });
    window.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref())?;
    keydown.forget();

    let move_app = Rc::clone(&app);
    let move_viewport = Rc::clone(&viewport);
    let move_terminal = terminal.clone();
    let mousemove = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
        let area = move_viewport.get();
        let (column, row) = pointer_cell(&event, &move_terminal, area);
        let target = {
            let app = move_app.borrow();
            svetsec_ui::help_target_at(area, column, row, &app)
        };
        let _ = move_app.borrow_mut().update(Message::Hover(target));
    });
    terminal.add_event_listener_with_callback("mousemove", mousemove.as_ref().unchecked_ref())?;
    mousemove.forget();

    let click_app = Rc::clone(&app);
    let click_viewport = Rc::clone(&viewport);
    let click_terminal = terminal.clone();
    let click = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
        if event.button() != 0 || browser_has_text_selection() {
            return;
        }
        let area = click_viewport.get();
        let (column, row) = pointer_cell(&event, &click_terminal, area);
        activate_at(Rc::clone(&click_app), area, column, row);
    });
    terminal.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
    click.forget();

    let leave_app = app;
    let mouseleave = Closure::<dyn FnMut(MouseEvent)>::new(move |_: MouseEvent| {
        let _ = leave_app.borrow_mut().update(Message::Hover(None));
    });
    terminal.add_event_listener_with_callback("mouseleave", mouseleave.as_ref().unchecked_ref())?;
    mouseleave.forget();
    Ok(())
}

fn browser_key_code(key: &str) -> Option<KeyCode> {
    match key {
        "ArrowUp" => Some(KeyCode::Up),
        "ArrowDown" => Some(KeyCode::Down),
        "ArrowLeft" => Some(KeyCode::Left),
        "ArrowRight" => Some(KeyCode::Right),
        "Home" => Some(KeyCode::Home),
        "End" => Some(KeyCode::End),
        "Enter" => Some(KeyCode::Enter),
        "Escape" => Some(KeyCode::Esc),
        "Tab" => Some(KeyCode::Tab),
        _ => {
            let mut characters = key.chars();
            let character = characters.next()?;
            characters
                .next()
                .is_none()
                .then_some(KeyCode::Char(character))
        }
    }
}

fn handle_key(app: Rc<RefCell<App>>, code: KeyCode) {
    let read_only_article = {
        let app = app.borrow();
        app.selected() == Tab::Articles && app.opened_article().is_some()
    };
    if !read_only_article && char_is(&code, &['a', 'ф']) {
        begin_login(app);
        return;
    }
    let selected = app.borrow().selected();
    if selected == Tab::Projects {
        let message = if matches!(&code, KeyCode::Up) || char_is(&code, &['k', 'л']) {
            Some(Message::PreviousProject)
        } else if matches!(&code, KeyCode::Down) || char_is(&code, &['j', 'о']) {
            Some(Message::NextProject)
        } else if matches!(&code, KeyCode::Enter) || char_is(&code, &['o', 'щ']) {
            Some(Message::OpenSelectedProject)
        } else {
            None
        };
        if let Some(message) = message {
            if let Some(effect) = app.borrow_mut().update(message) {
                apply_effect(effect);
            }
            return;
        }
    }
    if selected == Tab::Articles {
        let article_open = app.borrow().opened_article().is_some();
        if article_open && char_is(&code, &['x', 'ч']) && app.borrow().python_output().is_some() {
            let _ = app.borrow_mut().update(Message::DismissPythonOutput);
            return;
        }
        if article_open && char_is(&code, &['p', 'з']) {
            run_article_python(app);
            return;
        }
        if article_open && char_is(&code, &['c', 'с']) {
            copy_article_code(app, None);
            return;
        }
        if matches!(&code, KeyCode::Up) || char_is(&code, &['k', 'л']) {
            let message = if app.borrow().opened_article().is_some() {
                Message::ScrollArticleUp
            } else {
                Message::PreviousArticle
            };
            let _ = app.borrow_mut().update(message);
            return;
        }
        if matches!(&code, KeyCode::Down) || char_is(&code, &['j', 'о']) {
            let message = if app.borrow().opened_article().is_some() {
                Message::ScrollArticleDown
            } else {
                Message::NextArticle
            };
            let _ = app.borrow_mut().update(message);
            return;
        }
        if !article_open && (matches!(&code, KeyCode::Enter) || char_is(&code, &['o', 'щ'])) {
            load_selected_article(app);
            return;
        }
        if matches!(&code, KeyCode::Esc) && article_open {
            let _ = app.borrow_mut().update(Message::CloseArticle);
            return;
        }
        if char_is(&code, &['e', 'у']) {
            if app.borrow().authenticated() {
                open_article_editor(&app, false);
            } else {
                begin_login(app);
            }
            return;
        }
        if char_is(&code, &['n', 'т']) {
            if app.borrow().authenticated() {
                open_article_editor(&app, true);
            } else {
                begin_login(app);
            }
            return;
        }
        if char_is(&code, &['f', 'а']) {
            load_articles(app, true);
            return;
        }
    }
    let was_articles = selected == Tab::Articles;
    let language_toggled = char_is(&code, &['r', 'к']);
    if let Some(effect) = app.borrow_mut().update(message_for_key(code)) {
        apply_effect(effect);
    }
    if !was_articles && app.borrow().selected() == Tab::Articles {
        load_articles(Rc::clone(&app), false);
    }
    if language_toggled {
        store_language(app.borrow().language());
        let opened_slug = app
            .borrow()
            .opened_article()
            .map(|article| article.slug.clone());
        if let Some(slug) = opened_slug {
            load_article_slug(Rc::clone(&app), slug);
        } else if app.borrow().selected() == Tab::Articles {
            load_articles(Rc::clone(&app), true);
        }
        schedule_language_notice_hide(app);
    }
}

fn stored_language() -> Option<svetsec_core::Language> {
    let value = web_sys::window()?
        .local_storage()
        .ok()??
        .get_item("svetsec_language")
        .ok()??;
    svetsec_core::Language::from_code(&value)
}

fn store_language(language: svetsec_core::Language) {
    if let Some(storage) = web_sys::window()
        .and_then(|window| window.local_storage().ok())
        .flatten()
    {
        let _ = storage.set_item("svetsec_language", language.path_code());
    }
}

fn pointer_cell(
    event: &MouseEvent,
    terminal: &web_sys::Element,
    area: ratzilla::ratatui::layout::Rect,
) -> (u16, u16) {
    if let Some(document) = web_sys::window().and_then(|window| window.document())
        && let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid")
        && let Ok(Some(first_row)) = grid.query_selector("pre")
        && let Ok(Some(first_cell)) = first_row.query_selector("span")
    {
        let row_bounds = first_row.get_bounding_client_rect();
        let cell_bounds = first_cell.get_bounding_client_rect();
        if row_bounds.height() > 0.0 && cell_bounds.width() > 0.0 {
            return (
                grid_cell_axis(
                    f64::from(event.client_x()) - cell_bounds.left(),
                    cell_bounds.width(),
                    area.width,
                ),
                grid_cell_axis(
                    f64::from(event.client_y()) - row_bounds.top(),
                    row_bounds.height(),
                    area.height,
                ),
            );
        }
    }
    let bounds = terminal.get_bounding_client_rect();
    (
        grid_axis(
            f64::from(event.client_x()) - bounds.left(),
            bounds.width(),
            area.width,
        ),
        grid_axis(
            f64::from(event.client_y()) - bounds.top(),
            bounds.height(),
            area.height,
        ),
    )
}

fn grid_cell_axis(offset: f64, cell_extent: f64, cells: u16) -> u16 {
    if cell_extent <= 0.0 || cells == 0 {
        return 0;
    }
    ((offset.max(0.0) / cell_extent).floor() as u16).min(cells.saturating_sub(1))
}

fn grid_axis(offset: f64, extent: f64, cells: u16) -> u16 {
    if extent <= 0.0 || cells == 0 {
        return 0;
    }
    ((offset.max(0.0) / extent * f64::from(cells)) as u16).min(cells - 1)
}

fn activate_at(
    app: Rc<RefCell<App>>,
    area: ratzilla::ratatui::layout::Rect,
    column: u16,
    row: u16,
) {
    let target = {
        let app = app.borrow();
        svetsec_ui::help_target_at(area, column, row, &app)
    };
    let _ = app.borrow_mut().update(Message::Hover(target));
    if svetsec_ui::article_back_area(area, &app.borrow())
        .is_some_and(|area| area.contains((column, row).into()))
    {
        let _ = app.borrow_mut().update(Message::CloseArticle);
        return;
    }
    if svetsec_ui::resume_link_area(area, &app.borrow())
        .is_some_and(|area| area.contains((column, row).into()))
    {
        let _ = ratzilla::utils::open_url("/resume", true);
        return;
    }
    let project = {
        let app = app.borrow();
        svetsec_ui::project_at(area, column, row, &app)
    };
    if let Some(index) = project {
        let _ = app.borrow_mut().update(Message::SelectProject(index));
        if let Some(effect) = app.borrow_mut().update(Message::OpenSelectedProject) {
            apply_effect(effect);
        }
        return;
    }
    if svetsec_ui::python_output_close_area(area, &app.borrow())
        .is_some_and(|area| area.contains((column, row).into()))
    {
        let _ = app.borrow_mut().update(Message::DismissPythonOutput);
        return;
    }
    let code_action = {
        let app = app.borrow();
        svetsec_ui::code_action_at(area, column, row, &app)
    };
    if let Some(action) = code_action {
        let _ = app
            .borrow_mut()
            .update(Message::SelectArticleCursor(action.row()));
        match action {
            svetsec_ui::CodeBlockAction::Run { block, .. } => {
                run_article_python_block(app, block);
            }
            svetsec_ui::CodeBlockAction::Copy { block, .. } => {
                copy_article_code(app, Some(block));
            }
        }
        return;
    }
    let article_position = {
        let app = app.borrow();
        svetsec_ui::article_position_at(area, column, row, &app)
    };
    if let Some((row, column)) = article_position {
        let _ = app
            .borrow_mut()
            .update(Message::SelectArticlePosition { row, column });
        return;
    }
    let article = {
        let app = app.borrow();
        svetsec_ui::article_at(area, column, row, &app)
    };
    if let Some(index) = article {
        let _ = app.borrow_mut().update(Message::SelectArticle(index));
        load_selected_article(app);
        return;
    }
    if let Some(tab) = svetsec_ui::tab_at(area, column, row) {
        let _ = app.borrow_mut().update(Message::SelectTab(tab));
        if tab == Tab::Articles {
            load_articles(app, false);
        }
    }
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

    let placements = svetsec_ui::native_image_placements(area, app);
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
        let source = if placement.source.starts_with('/') {
            placement.source.to_owned()
        } else {
            format!("/api/github/assets/{}", placement.source)
        };
        image.set_attribute("src", &source)?;
        image.set_attribute("alt", placement.alt)?;
        image.set_attribute(
            "style",
            &format!(
                "display:block;left:{left}px;top:{top}px;width:{width}px;height:{height}px;\
                 clip-path:inset({clip_top}px {clip_right}px {clip_bottom}px 0px);{}",
                if placement.rounded {
                    "border-radius:50%;"
                } else {
                    ""
                }
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
        KeyCode::Char('3') => Message::SelectTab(Tab::Projects),
        KeyCode::Char('4') => Message::SelectTab(Tab::Info),
        KeyCode::Char('r' | 'к') => Message::ToggleLanguage,
        KeyCode::Char('g' | 'п') => Message::BeginSiteShortcut,
        KeyCode::Char('x' | 'ч') => Message::CompleteSiteShortcut,
        _ => Message::CancelShortcut,
    }
}

fn apply_effect(effect: Effect) {
    match effect {
        Effect::OpenUrl(url) => {
            let _ = ratzilla::utils::open_url(url, true);
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

fn animate_ui(app: Rc<RefCell<App>>) {
    spawn_local(async move {
        let mut advance_skeleton = false;
        loop {
            TimeoutFuture::new(50).await;
            advance_skeleton = !advance_skeleton;
            if advance_skeleton
                && (app.borrow().articles_loading() || app.borrow().article_loading())
            {
                let _ = app.borrow_mut().update(Message::AdvanceSkeleton);
            }
            if app.borrow().article_animation_active() {
                let _ = app.borrow_mut().update(Message::AdvanceArticleAnimation);
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
    let language = app.borrow().language();
    spawn_local(async move {
        match fetch_articles(force, language).await {
            Ok((articles, create_url)) => {
                let mut app = app.borrow_mut();
                app.set_articles(articles);
                app.set_article_create_url(create_url);
            }
            Err(_) => {
                let error = match app.borrow().language() {
                    svetsec_core::Language::En => "Could not load articles.",
                    svetsec_core::Language::Ru => "Не удалось загрузить статьи.",
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
    load_article_slug(app, slug);
}

fn load_article_slug(app: Rc<RefCell<App>>, slug: String) {
    app.borrow_mut().begin_article_load();
    let language = app.borrow().language();
    spawn_local(async move {
        match fetch_article(&slug, language).await {
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

fn run_article_python(app: Rc<RefCell<App>>) {
    let Some(block) = app.borrow().focused_code_block() else {
        return;
    };
    run_article_python_block(app, block.index);
}

fn run_article_python_block(app: Rc<RefCell<App>>, block_index: usize) {
    if app.borrow().python_running()
        || !app
            .borrow()
            .article_code_block(block_index)
            .is_some_and(|block| block.executable())
    {
        return;
    }
    let Some(slug) = app
        .borrow()
        .opened_article()
        .map(|article| article.slug.clone())
    else {
        return;
    };
    let language = app.borrow().language();
    app.borrow_mut().begin_python_run();
    spawn_local(async move {
        let result = request_json(
            "POST",
            &format!(
                "/api/github/articles/{slug}/python/{block_index}?lang={}",
                language.path_code()
            ),
            None,
        )
        .await
        .and_then(|json| {
            js_sys::Reflect::get(&json, &JsValue::from_str("output"))?
                .as_string()
                .ok_or_else(|| JsValue::from_str("Python output missing"))
        });
        app.borrow_mut().finish_python_run(match result {
            Ok(output) if output.trim().is_empty() => "(no output)".into(),
            Ok(output) => output,
            Err(_) => "Python execution is unavailable.".into(),
        });
    });
}

fn copy_article_code(app: Rc<RefCell<App>>, block_index: Option<usize>) {
    let block = block_index
        .and_then(|index| app.borrow().article_code_block(index))
        .or_else(|| app.borrow().focused_code_block());
    let Some(block) = block.filter(|block| !block.animated() && !block.code.is_empty()) else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    let promise = window.navigator().clipboard().write_text(&block.code);
    spawn_local(async move {
        let _ = JsFuture::from(promise).await;
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

async fn fetch_articles(
    refresh: bool,
    language: svetsec_core::Language,
) -> Result<(Vec<ArticleSummary>, String), JsValue> {
    let url = if refresh {
        format!(
            "/api/github/articles?refresh=1&lang={}",
            language.path_code()
        )
    } else {
        format!("/api/github/articles?lang={}", language.path_code())
    };
    let json = request_json("GET", &url, None).await?;
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
            labels: string_array(&value, "labels"),
        });
    }
    let create_url = js_sys::Reflect::get(&json, &JsValue::from_str("create_url"))?
        .as_string()
        .unwrap_or_default();
    Ok((articles, create_url))
}

async fn fetch_article(
    slug: &str,
    language: svetsec_core::Language,
) -> Result<ArticleContent, JsValue> {
    let json = request_json(
        "GET",
        &format!("/api/github/articles/{slug}?lang={}", language.path_code()),
        None,
    )
    .await?;
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
        labels: string_array(&json, "labels"),
    })
}

fn string_array(value: &JsValue, field: &str) -> Vec<String> {
    js_sys::Reflect::get(value, &JsValue::from_str(field))
        .ok()
        .map(|value| {
            js_sys::Array::from(&value)
                .iter()
                .filter_map(|value| value.as_string())
                .collect()
        })
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use ratzilla::event::KeyCode;
    use svetsec_core::{App, ArticleContent, Message, Tab};

    use super::{
        WebRoute, browser_key_code, cell_contains_selectable_text, grid_axis, grid_cell_axis,
        selection_runs,
    };

    #[test]
    fn browser_keys_support_both_layouts() {
        assert_eq!(browser_key_code("R"), Some(KeyCode::Char('R')));
        assert_eq!(browser_key_code("К"), Some(KeyCode::Char('К')));
        assert_eq!(browser_key_code("ArrowRight"), Some(KeyCode::Right));
        assert_eq!(browser_key_code("Home"), Some(KeyCode::Home));
        assert_eq!(browser_key_code("End"), Some(KeyCode::End));
    }

    #[test]
    fn pointer_coordinates_follow_the_current_grid() {
        assert_eq!(grid_axis(500.0, 1_000.0, 100), 50);
        assert_eq!(grid_axis(1_000.0, 1_000.0, 100), 99);
        assert_eq!(grid_axis(10.0, 0.0, 100), 0);
        assert_eq!(grid_cell_axis(55.0, 10.0, 100), 5);
        assert_eq!(grid_cell_axis(2_000.0, 10.0, 100), 99);
    }

    #[test]
    fn selection_ignores_terminal_chrome_and_animation_cells() {
        assert!(cell_contains_selectable_text("Article text"));
        assert!(cell_contains_selectable_text("print(42)"));
        assert!(!cell_contains_selectable_text("   "));
        assert!(!cell_contains_selectable_text("╭────╮"));
        assert!(!cell_contains_selectable_text("⠋⠙⠹"));
    }

    #[test]
    fn selection_runs_keep_words_together_and_split_wide_gaps() {
        let cells = "alpha beta    telemetry"
            .chars()
            .map(|character| character.to_string())
            .collect::<Vec<_>>();
        assert_eq!(selection_runs(&cells), vec![(0, 9), (14, 22)]);

        let cells = ["article", " ", "text", "╭", "status"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(selection_runs(&cells), vec![(0, 2), (4, 4)]);
    }

    #[test]
    fn web_routes_round_trip_and_reject_unsafe_slugs() {
        assert_eq!(WebRoute::from_path("/"), WebRoute::Main);
        assert_eq!(WebRoute::from_path("/articles"), WebRoute::Articles);
        assert_eq!(WebRoute::from_path("/projects"), WebRoute::Projects);
        assert_eq!(
            WebRoute::from_path("/articles/hello-world"),
            WebRoute::Article("hello-world".into())
        );
        assert_eq!(WebRoute::from_path("/articles/../secret"), WebRoute::Main);
        assert_eq!(
            WebRoute::Article("hello-world".into()).path(),
            "/articles/hello-world"
        );
        assert_eq!(WebRoute::Projects.path(), "/projects");

        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.set_opened_article(ArticleContent {
            slug: "hello-world".into(),
            title: "Hello".into(),
            markdown: "# Hello".into(),
            images: Vec::new(),
            labels: Vec::new(),
        });
        assert_eq!(
            WebRoute::for_app(&app),
            WebRoute::Article("hello-world".into())
        );
    }
}
