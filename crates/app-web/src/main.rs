use std::{
    cell::{Cell, RefCell},
    io,
    rc::Rc,
};

use gloo_timers::future::TimeoutFuture;
use ratzilla::{
    CellSized, DomBackend,
    event::KeyCode,
    ratatui::{
        Terminal,
        backend::{Backend, ClearType, WindowSize},
        buffer::Cell as TerminalCell,
        layout::{Position, Size},
    },
};
use svetsec_core::{
    App, ArticleContent, ArticleImage, ArticleSummary, Comment, Effect, HelpTarget, Message, Tab,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    HtmlInputElement, HtmlTextAreaElement, KeyboardEvent, MouseEvent, Request, RequestCredentials,
    RequestInit, Response,
};

/// `DomBackend::size` uses the physical screen on mobile user agents. That is
/// larger than the actual terminal whenever the bottom touch controls are
/// visible, so Ratatui renders a desktop-sized grid and CSS has to squash it
/// into the phone viewport. Keep the DOM renderer, but report the dimensions
/// of its real parent element to Ratatui instead.
struct ViewportDomBackend {
    inner: DomBackend,
    parent_id: &'static str,
}

impl ViewportDomBackend {
    fn new_by_id(parent_id: &'static str) -> Result<Self, ratzilla::error::Error> {
        Ok(Self {
            inner: DomBackend::new_by_id(parent_id)?,
            parent_id,
        })
    }

    fn viewport_size(&self) -> io::Result<Size> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| io::Error::other("document unavailable"))?;
        let parent = document
            .get_element_by_id(self.parent_id)
            .ok_or_else(|| io::Error::other("terminal element unavailable"))?;
        let bounds = parent.get_bounding_client_rect();
        let (cell_width, cell_height) = self.inner.cell_size_css_px();
        Ok(terminal_grid_size(
            bounds.width(),
            bounds.height(),
            cell_width,
            cell_height,
        ))
    }
}

fn terminal_grid_size(
    viewport_width: f64,
    viewport_height: f64,
    cell_width: f32,
    cell_height: f32,
) -> Size {
    let cells = |viewport: f64, cell: f32| {
        if !viewport.is_finite() || !cell.is_finite() || viewport <= 0.0 || cell <= 0.0 {
            return 1;
        }
        (viewport / f64::from(cell))
            .floor()
            .clamp(1.0, f64::from(u16::MAX)) as u16
    };
    Size::new(
        cells(viewport_width, cell_width),
        cells(viewport_height, cell_height),
    )
}

impl Backend for ViewportDomBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a TerminalCell)>,
    {
        self.inner.draw(content)
    }

    fn append_lines(&mut self, count: u16) -> Result<(), Self::Error> {
        self.inner.append_lines(count)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.viewport_size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        let columns_rows = self.viewport_size()?;
        let (cell_width, cell_height) = self.inner.cell_size_css_px();
        Ok(WindowSize {
            columns_rows,
            pixels: Size::new(
                (f32::from(columns_rows.width) * cell_width) as u16,
                (f32::from(columns_rows.height) * cell_height) as u16,
            ),
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

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
    authenticated: bool,
    username: Option<String>,
    avatar_url: Option<String>,
    telegram_login_enabled: bool,
    can_moderate_comments: bool,
    keyboard_hints_hidden: bool,
    comments_len: usize,
    comments_loading: bool,
    comments_error: bool,
    language_notice: bool,
    image_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RenderSignature {
    dom: DomSignature,
    selected_article: usize,
    skeleton_phase: u16,
    article_animation_phase: u16,
    awaiting_site_key: bool,
    awaiting_article_g: bool,
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
            authenticated: app.authenticated(),
            username: app.username().map(str::to_owned),
            avatar_url: app.avatar_url().map(str::to_owned),
            telegram_login_enabled: app.telegram_login_enabled(),
            can_moderate_comments: app.can_moderate_comments(),
            keyboard_hints_hidden: app.keyboard_hints_hidden(),
            comments_len: app.comments().len(),
            comments_loading: app.comments_loading(),
            comments_error: app.comments_error().is_some(),
            language_notice: app.language_notice(),
            image_count: app
                .opened_article()
                .map_or(usize::from(app.profile_image().is_some()), |article| {
                    article.images.len()
                }),
        }
    }
}

impl RenderSignature {
    fn new(area: ratzilla::ratatui::layout::Rect, app: &App) -> Self {
        Self {
            dom: DomSignature::new(area, app),
            selected_article: app.selected_article_index(),
            skeleton_phase: app.skeleton_phase(),
            article_animation_phase: app.article_animation_phase(),
            awaiting_site_key: app.awaiting_site_key(),
            awaiting_article_g: app.awaiting_article_g(),
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
                    Ok(article) => {
                        app.borrow_mut().set_opened_article(article);
                        load_comments(Rc::clone(&app));
                    }
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
    initial_app.set_keyboard_hints_hidden(mobile_controls_layout());
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
    let browser_image_ids = Rc::new(RefCell::new(Vec::<String>::new()));
    let dom_signature = Rc::new(RefCell::new(None::<DomSignature>));
    let render_signature = Rc::new(RefCell::new(None::<RenderSignature>));
    let scroll_settle_generation = Rc::new(Cell::new(0_u32));
    let route_state = Rc::new(RefCell::new(RouteState {
        current: initial_route.clone(),
        resolving: matches!(initial_route, WebRoute::Article(_)),
    }));
    let backend = ViewportDomBackend::new_by_id("terminal")?;
    let mut terminal = Terminal::new(backend)?;
    let full_redraw_required = Rc::new(Cell::new(false));
    install_full_redraw_on_resize(Rc::clone(&full_redraw_required), Rc::clone(&app))
        .map_err(|error| io::Error::other(format!("resize recovery setup failed: {error:?}")))?;
    install_browser_events(
        Rc::clone(&app),
        Rc::clone(&viewport),
        Rc::clone(&browser_image_ids),
        Rc::clone(&scroll_settle_generation),
    )
    .map_err(|error| io::Error::other(format!("browser event setup failed: {error:?}")))?;
    install_account_events(Rc::clone(&app))
        .map_err(|error| io::Error::other(format!("account event setup failed: {error:?}")))?;
    install_route_events(Rc::clone(&app), Rc::clone(&route_state))
        .map_err(|error| io::Error::other(format!("browser route setup failed: {error:?}")))?;

    load_session(Rc::clone(&app));
    animate_ui(Rc::clone(&app));
    apply_web_route(Rc::clone(&app), initial_route, Rc::clone(&route_state));

    let animation_frame = Rc::new(RefCell::new(None::<Closure<dyn FnMut()>>));
    let next_animation_frame = Rc::clone(&animation_frame);
    *animation_frame.borrow_mut() = Some(Closure::new(move || {
        if full_redraw_required.replace(false) {
            // DomBackend recreates every span after a resize, while Ratatui normally
            // sends only cells that differ from its previous buffer. Resetting that
            // buffer makes the rebuilt DOM receive static text as well as animation.
            terminal
                .clear()
                .expect("the DOM terminal should support a full redraw");
            *dom_signature.borrow_mut() = None;
            *render_signature.borrow_mut() = None;
        }

        let render_required = {
            let app = app.borrow();
            let next = RenderSignature::new(viewport.get(), &app);
            render_signature.borrow().as_ref() != Some(&next)
        };
        if render_required {
            terminal
                .draw(|frame| {
                    viewport.set(frame.area());
                    let app_handle = Rc::clone(&app);
                    let mut app = app.borrow_mut();
                    app.set_article_viewport_rows(svetsec_ui::article_viewport_rows(frame.area()));
                    let next_render_signature = RenderSignature::new(frame.area(), &app);
                    let signature = next_render_signature.dom.clone();
                    let previous_signature = dom_signature.borrow();
                    let changed = previous_signature.as_ref() != Some(&signature);
                    let structural_transition = previous_signature
                        .as_ref()
                        .is_some_and(|previous| structural_dom_transition(previous, &signature));
                    let navigation_only_transition =
                        previous_signature.as_ref().is_some_and(|previous| {
                            article_navigation_only_transition(previous, &signature)
                        });
                    drop(previous_signature);
                    if structural_transition {
                        let _ = reset_browser_transition_dom(&browser_image_ids);
                    }
                    svetsec_ui::render(frame, &app);
                    sync_browser_route(&app, &route_state);
                    if changed {
                        *dom_signature.borrow_mut() = Some(signature);
                        let sync_app = Rc::clone(&app_handle);
                        let sync_image_ids = Rc::clone(&browser_image_ids);
                        let area = frame.area();
                        // `spawn_local` schedules this work for the next microtask. The DOM
                        // backend has finished writing the Ratatui cells by then, while the
                        // browser has not painted an undecorated intermediate frame yet.
                        spawn_local(async move {
                            let app = sync_app.borrow();
                            if !navigation_only_transition {
                                let _ = sync_browser_tabs(area, &app);
                                let _ = sync_browser_articles(area, &app);
                                let _ = sync_browser_projects(area, &app);
                                let _ = sync_browser_navigation_links(area, &app);
                                let _ = sync_browser_account(area, &app);
                                let _ = sync_browser_comments(area, &app);
                            }
                            let _ = sync_browser_code_actions(area, &app);
                            if !navigation_only_transition {
                                let _ = sync_browser_output_close(area, &app);
                                let _ = sync_browser_comment_actions(area, &app);
                                let _ = sync_mobile_controls(&app);
                            }
                            let _ = sync_browser_native_scroll(area, &app);
                            if navigation_only_transition {
                                let _ = sync_browser_image_positions();
                            } else {
                                let _ = sync_browser_images(&app, area, &sync_image_ids);
                                let _ = sync_browser_text_selection();
                            }
                        });
                    }
                    *render_signature.borrow_mut() = Some(next_render_signature);
                })
                .expect("the DOM terminal should render an animation frame");
        }

        if let Some(callback) = next_animation_frame.borrow().as_ref()
            && let Some(window) = web_sys::window()
        {
            let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
        }
    }));

    let window = web_sys::window().ok_or_else(|| io::Error::other("window unavailable"))?;
    let animation_frame_ref = animation_frame.borrow();
    let callback = animation_frame_ref
        .as_ref()
        .ok_or_else(|| io::Error::other("animation callback unavailable"))?;
    window
        .request_animation_frame(callback.as_ref().unchecked_ref())
        .map_err(|error| io::Error::other(format!("animation setup failed: {error:?}")))?;
    Ok(())
}

fn install_full_redraw_on_resize(
    redraw_required: Rc<Cell<bool>>,
    app: Rc<RefCell<App>>,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let resize = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        app.borrow_mut()
            .set_keyboard_hints_hidden(mobile_controls_layout());
        redraw_required.set(true);
    });
    window.add_event_listener_with_callback("resize", resize.as_ref().unchecked_ref())?;
    resize.forget();
    Ok(())
}

fn mobile_controls_layout() -> bool {
    web_sys::window()
        .and_then(|window| {
            window
                .match_media("(max-width: 700px), (pointer: coarse)")
                .ok()
                .flatten()
        })
        .is_some_and(|query| query.matches())
}

fn structural_dom_transition(previous: &DomSignature, next: &DomSignature) -> bool {
    previous.area != next.area
        || previous.selected != next.selected
        || previous.language != next.language
        || previous.articles_loading != next.articles_loading
        || previous.article_loading != next.article_loading
        || previous.opened_slug != next.opened_slug
}

fn article_navigation_only_transition(previous: &DomSignature, next: &DomSignature) -> bool {
    if previous.selected != Tab::Articles
        || previous.opened_slug.is_none()
        || previous.article_loading
    {
        return false;
    }

    let navigation_changed = previous.article_scroll != next.article_scroll
        || previous.article_cursor != next.article_cursor
        || previous.article_cursor_column != next.article_cursor_column;
    let mut previous = previous.clone();
    let mut next = next.clone();
    previous.article_scroll = 0;
    previous.article_cursor = 0;
    previous.article_cursor_column = 0;
    next.article_scroll = 0;
    next.article_cursor = 0;
    next.article_cursor_column = 0;
    navigation_changed && previous == next
}

fn reset_browser_transition_dom(image_ids: &RefCell<Vec<String>>) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    if let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") {
        reset_browser_article_scroll_rows(&grid)?;
        grid.remove_attribute("data-block-selection")?;
        let decorated = grid.query_selector_all("span[class], span[data-text-block]")?;
        for index in 0..decorated.length() {
            if let Some(node) = decorated.item(index)
                && let Some(cell) = node.dyn_ref::<web_sys::Element>()
            {
                cell.remove_attribute("class")?;
                cell.remove_attribute("data-text-block")?;
            }
        }
    }
    if let Some(layer) = document.get_element_by_id("article-media-layer") {
        layer.set_text_content(None);
    }
    if let Some(comments) = document.get_element_by_id("web-comments-panel") {
        comments.set_attribute("hidden", "")?;
        comments.set_text_content(None);
    }
    image_ids.borrow_mut().clear();
    if let Some(terminal) = document.get_element_by_id("terminal") {
        terminal.remove_attribute("data-native-scroll")?;
        terminal.set_scroll_top(0);
    }
    if let Some(window) = web_sys::window() {
        window.scroll_to_with_x_and_y(0.0, 0.0);
    }
    if let Some(spacer) = document.get_element_by_id("web-article-scroll-spacer") {
        spacer.remove();
    }
    Ok(())
}

fn reset_browser_article_scroll_rows(grid: &web_sys::Element) -> Result<(), JsValue> {
    let wrappers = grid.query_selector_all(".web-article-scroll-row")?;
    for index in 0..wrappers.length() {
        let Some(node) = wrappers.item(index) else {
            continue;
        };
        let Some(wrapper) = node.dyn_ref::<web_sys::Element>() else {
            continue;
        };
        let Some(parent) = wrapper.parent_node() else {
            continue;
        };
        while let Some(child) = wrapper.first_child() {
            parent.insert_before(&child, Some(wrapper))?;
        }
        parent.remove_child(wrapper)?;
    }
    let rows =
        grid.query_selector_all("pre.web-article-scroll-line, pre.web-article-static-line")?;
    for index in 0..rows.length() {
        if let Some(node) = rows.item(index)
            && let Some(row) = node.dyn_ref::<web_sys::Element>()
        {
            row.remove_attribute("class")?;
            row.remove_attribute("data-terminal-row")?;
        }
    }
    grid.remove_attribute("data-article-scroll-layout")?;
    grid.remove_attribute("data-rendered-article-scroll")?;
    if let Some(grid) = grid.dyn_ref::<web_sys::HtmlElement>() {
        grid.style().remove_property("--article-scroll-offset")?;
    }
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
                let Some(cell) = terminal_cell(&grid, row, column)? else {
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
            let Some(cell) = terminal_cell(&grid, area.top(), column)? else {
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
        back.set_text_content(Some("← Articles"));
    }
    if let Some(comment) = controls.query_selector("[data-article-action=\"comment\"]")? {
        comment.set_text_content(Some(if app.signed_in() {
            "Comment"
        } else {
            "Sign in"
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

fn sync_browser_account(area: ratzilla::ratatui::layout::Rect, app: &App) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(account) = document.get_element_by_id("web-account") else {
        return Ok(());
    };
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        account.set_attribute("hidden", "")?;
        return Ok(());
    };
    let account_area = svetsec_ui::account_area(area);
    if account_area.is_empty() {
        account.set_attribute("hidden", "")?;
        return Ok(());
    }
    position_dom_area(&account, &grid, account_area)?;
    account.remove_attribute("hidden")?;

    let signed_in = app.signed_in();
    if let Some(menu) = document.get_element_by_id("account-menu") {
        menu.set_attribute("data-signed-in", if signed_in { "true" } else { "false" })?;
        menu.set_attribute(
            "data-owner",
            if app.authenticated() { "true" } else { "false" },
        )?;
    }
    if let Some(name) = document.get_element_by_id("account-name") {
        let label = if app.authenticated() {
            "@svetsec".to_owned()
        } else if let Some(username) = app.username() {
            format!("@{username}")
        } else {
            "Login".into()
        };
        name.set_text_content(Some(&label));
    }
    if let Some(avatar) = document.get_element_by_id("account-avatar") {
        if let Some(source) = app.avatar_url() {
            avatar.set_attribute("src", source)?;
            avatar.set_attribute(
                "alt",
                app.username().unwrap_or(if app.authenticated() {
                    "svetsec"
                } else {
                    "user"
                }),
            )?;
            avatar.remove_attribute("hidden")?;
        } else {
            avatar.set_attribute("hidden", "")?;
            avatar.remove_attribute("src")?;
        }
    }

    let login_url = telegram_login_url();
    configure_telegram_link(
        &document,
        "account-telegram-login",
        app.telegram_login_enabled(),
        &login_url,
    )?;
    configure_telegram_link(
        &document,
        "telegram-login",
        app.telegram_login_enabled(),
        &login_url,
    )?;

    for (id, text) in [
        ("account-telegram-login", "Continue with Telegram"),
        ("account-avatar-label", "Change avatar"),
        ("account-logout", "Log out"),
    ] {
        if let Some(element) = document.get_element_by_id(id) {
            element.set_text_content(Some(text));
        }
    }
    if let Some(help) = document.get_element_by_id("telegram-login-help") {
        help.set_text_content(Some(match (app.telegram_login_enabled(), app.language()) {
            (true, svetsec_core::Language::En) => "No separate site password is stored.",
            (true, svetsec_core::Language::Ru) => "Отдельный пароль сайта не сохраняется.",
            (false, svetsec_core::Language::En) => {
                "Telegram login is waiting for server configuration."
            }
            (false, svetsec_core::Language::Ru) => "Вход через Telegram ожидает настройки сервера.",
        }));
    }
    Ok(())
}

fn configure_telegram_link(
    document: &web_sys::Document,
    id: &str,
    enabled: bool,
    url: &str,
) -> Result<(), JsValue> {
    let Some(link) = document.get_element_by_id(id) else {
        return Ok(());
    };
    link.set_attribute("aria-disabled", if enabled { "false" } else { "true" })?;
    if enabled {
        link.set_attribute("href", url)?;
    } else {
        link.remove_attribute("href")?;
    }
    Ok(())
}

fn telegram_login_url() -> String {
    let path = web_sys::window()
        .and_then(|window| window.location().pathname().ok())
        .unwrap_or_else(|| "/".into());
    let encoded = js_sys::encode_uri_component(&path)
        .as_string()
        .unwrap_or_else(|| "%2F".into());
    format!("/api/auth/telegram/start?next={encoded}")
}

fn sync_browser_comments(area: ratzilla::ratatui::layout::Rect, app: &App) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(panel) = document.get_element_by_id("web-comments-panel") else {
        return Ok(());
    };
    let Some(viewport) = svetsec_ui::comments_viewport_area(area, app) else {
        panel.set_attribute("hidden", "")?;
        panel.set_text_content(None);
        return Ok(());
    };
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        panel.set_attribute("hidden", "")?;
        return Ok(());
    };
    let scroll_top = panel.scroll_top();
    position_dom_area(&panel, &grid, viewport)?;
    panel.set_attribute(
        "aria-label",
        match app.language() {
            svetsec_core::Language::En => "Comments",
            svetsec_core::Language::Ru => "Комментарии",
        },
    )?;
    panel.set_text_content(None);
    if app.comments_loading() || app.comments_error().is_some() || app.comments().is_empty() {
        let message = document.create_element("p")?;
        let text = if app.comments_loading() {
            match app.language() {
                svetsec_core::Language::En => "Loading comments…",
                svetsec_core::Language::Ru => "Загрузка комментариев…",
            }
        } else if let Some(error) = app.comments_error() {
            error
        } else {
            match app.language() {
                svetsec_core::Language::En => "No comments yet.",
                svetsec_core::Language::Ru => "Комментариев пока нет.",
            }
        };
        message.set_text_content(Some(text));
        panel.append_child(&message)?;
    } else {
        for comment in app.comments() {
            append_comment_entry(&document, &panel, comment, app.can_moderate_comments())?;
        }
    }
    panel.remove_attribute("hidden")?;
    panel.set_scroll_top(scroll_top);
    Ok(())
}

fn append_comment_entry(
    document: &web_sys::Document,
    parent: &web_sys::Element,
    comment: &Comment,
    can_delete: bool,
) -> Result<(), JsValue> {
    let entry = document.create_element("article")?;
    let heading = document.create_element("div")?;
    let author = document.create_element("strong")?;
    let body = document.create_element("div")?;
    entry.set_class_name("comment-entry");
    heading.set_class_name("comment-entry-heading");
    author.set_text_content(Some(&format!("@{}", comment.author)));
    body.set_text_content(Some(&comment.body));
    heading.append_child(&author)?;
    if can_delete {
        let button = document.create_element("button")?;
        let label = "Delete comment";
        button.set_class_name("comment-delete");
        button.set_attribute("type", "button")?;
        button.set_attribute("data-comment-delete", &comment.id.to_string())?;
        button.set_attribute("aria-label", label)?;
        button.set_attribute("title", label)?;
        button.set_text_content(Some("×"));
        heading.append_child(&button)?;
    }
    entry.append_child(&heading)?;
    entry.append_child(&body)?;
    parent.append_child(&entry)?;
    Ok(())
}

fn position_dom_area(
    element: &web_sys::Element,
    grid: &web_sys::Element,
    area: ratzilla::ratatui::layout::Rect,
) -> Result<(), JsValue> {
    let Some(first_row) = grid.query_selector("pre")? else {
        return Ok(());
    };
    let Some(first_cell) = first_row.query_selector("span")? else {
        return Ok(());
    };
    let row = first_row.get_bounding_client_rect();
    let cell = first_cell.get_bounding_client_rect();
    let Some(element) = element.dyn_ref::<web_sys::HtmlElement>() else {
        return Ok(());
    };
    let style = element.style();
    style.set_property(
        "left",
        &format!("{}px", row.left() + f64::from(area.x) * cell.width()),
    )?;
    style.set_property(
        "top",
        &format!("{}px", row.top() + f64::from(area.y) * row.height()),
    )?;
    style.set_property(
        "width",
        &format!("{}px", f64::from(area.width) * cell.width()),
    )?;
    style.set_property(
        "height",
        &format!("{}px", f64::from(area.height) * row.height()),
    )?;
    Ok(())
}

fn sync_browser_native_scroll(
    area: ratzilla::ratatui::layout::Rect,
    app: &App,
) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(terminal) = document.get_element_by_id("terminal") else {
        return Ok(());
    };
    let article_open =
        app.selected() == Tab::Articles && app.opened_article().is_some() && !app.article_loading();
    if !article_open {
        terminal.remove_attribute("data-native-scroll")?;
        terminal.set_scroll_top(0);
        if let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") {
            reset_browser_article_scroll_rows(&grid)?;
        }
        if let Some(spacer) = document.get_element_by_id("web-article-scroll-spacer") {
            spacer.remove();
        }
        return Ok(());
    }

    terminal.set_attribute("data-native-scroll", "true")?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    let Some(row_height) = browser_terminal_row_height(&grid)? else {
        return Ok(());
    };
    ensure_browser_article_scroll_rows(&document, &grid, svetsec_ui::article_viewport_area(area))?;

    let spacer = match document.get_element_by_id("web-article-scroll-spacer") {
        Some(spacer) => spacer,
        None => {
            let spacer = document.create_element("div")?;
            spacer.set_attribute("id", "web-article-scroll-spacer")?;
            terminal.append_child(&spacer)?;
            spacer
        }
    };
    let spacer_style = format!(
        "height:{}px",
        f64::from(app.article_scroll_limit()) * row_height
    );
    if spacer.get_attribute("style").as_deref() != Some(&spacer_style) {
        spacer.set_attribute("style", &spacer_style)?;
    }

    let mut scroll_top = f64::from(terminal.scroll_top().max(0));
    if native_scroll_row(scroll_top, row_height, app.article_scroll_limit()) != app.article_scroll()
    {
        let desired = rendered_scroll_top(app.article_scroll(), row_height);
        terminal.set_scroll_top(desired.round() as i32);
        scroll_top = desired;
    }
    grid.set_attribute(
        "data-rendered-article-scroll",
        &app.article_scroll().to_string(),
    )?;
    apply_browser_article_scroll_offset(&grid, scroll_top, app.article_scroll(), row_height)?;
    Ok(())
}

fn rendered_scroll_top(article_scroll: u16, row_height: f64) -> f64 {
    f64::from(article_scroll) * row_height
}

fn browser_terminal_row_height(grid: &web_sys::Element) -> Result<Option<f64>, JsValue> {
    if let Some(height) = grid
        .get_attribute("data-terminal-row-height")
        .and_then(|height| height.parse::<f64>().ok())
        .filter(|height| *height > 0.0)
    {
        return Ok(Some(height));
    }
    let Some(first_row) = grid.query_selector("pre")? else {
        return Ok(None);
    };
    let height = first_row.get_bounding_client_rect().height();
    if height <= 0.0 {
        return Ok(None);
    }
    grid.set_attribute("data-terminal-row-height", &height.to_string())?;
    Ok(Some(height))
}

fn native_scroll_row(scroll_top: f64, row_height: f64, limit: u16) -> u16 {
    if row_height <= 0.0 {
        return 0;
    }
    ((scroll_top.max(0.0) / row_height).floor() as u16).min(limit)
}

fn article_scroll_offset(scroll_top: f64, rendered_row: u16, row_height: f64) -> f64 {
    if row_height <= 0.0 {
        return 0.0;
    }
    (scroll_top - rendered_scroll_top(rendered_row, row_height)).clamp(-row_height, row_height)
}

fn apply_browser_article_scroll_offset(
    grid: &web_sys::Element,
    scroll_top: f64,
    rendered_row: u16,
    row_height: f64,
) -> Result<(), JsValue> {
    let Some(grid) = grid.dyn_ref::<web_sys::HtmlElement>() else {
        return Ok(());
    };
    let offset = -article_scroll_offset(scroll_top, rendered_row, row_height);
    grid.style()
        .set_property("--article-scroll-offset", &format!("{offset:.3}px"))
}

fn ensure_browser_article_scroll_rows(
    document: &web_sys::Document,
    grid: &web_sys::Element,
    viewport: ratzilla::ratatui::layout::Rect,
) -> Result<(), JsValue> {
    let layout = format!(
        "{}:{}:{}:{}",
        viewport.x, viewport.y, viewport.width, viewport.height
    );
    if grid.get_attribute("data-article-scroll-layout").as_deref() == Some(&layout) {
        return Ok(());
    }
    reset_browser_article_scroll_rows(grid)?;

    let rows = grid.query_selector_all("pre")?;
    for row_index in 0..rows.length() {
        let Some(node) = rows.item(row_index) else {
            continue;
        };
        let Some(row) = node.dyn_ref::<web_sys::Element>() else {
            continue;
        };
        row.set_attribute("data-terminal-row", &row_index.to_string())?;
        let cells = row.query_selector_all("span:not(.web-article-scroll-row)")?;
        for column in 0..cells.length() {
            if let Some(node) = cells.item(column)
                && let Some(cell) = node.dyn_ref::<web_sys::Element>()
            {
                cell.set_attribute("data-terminal-cell", "")?;
                cell.set_attribute("data-terminal-column", &column.to_string())?;
            }
        }

        let row_number = row_index.min(u32::from(u16::MAX)) as u16;
        if row_number < viewport.top() || row_number >= viewport.bottom() {
            row.set_attribute("class", "web-article-static-line")?;
            continue;
        }
        row.set_attribute("class", "web-article-scroll-line")?;
        let wrapper = document.create_element("span")?;
        wrapper.set_attribute("class", "web-article-scroll-row")?;
        let first = cells.item(u32::from(viewport.left()));
        let Some(first) = first else {
            continue;
        };
        if let Some(first_cell) = first.dyn_ref::<web_sys::Element>() {
            let width = first_cell.get_bounding_client_rect().width() * f64::from(viewport.width);
            wrapper.set_attribute("style", &format!("width:{width:.3}px"))?;
        }
        row.insert_before(&wrapper, Some(&first))?;
        for column in viewport.left()..viewport.right() {
            if let Some(cell) = cells.item(u32::from(column)) {
                wrapper.append_child(&cell)?;
            }
        }
    }
    grid.set_attribute("data-article-scroll-layout", &layout)?;
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

fn sync_browser_comment_actions(
    area: ratzilla::ratatui::layout::Rect,
    app: &App,
) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
        return Ok(());
    };
    clear_cell_class(&grid, ".web-comment-action")?;
    for (_, area) in svetsec_ui::comment_action_areas(area, app) {
        set_area_class(&grid, area, "web-clickable web-comment-action")?;
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
            if let Some(cell) = terminal_cell(grid, row, column)? {
                cell.set_attribute("class", class_name)?;
            }
        }
    }
    Ok(())
}

fn terminal_cell(
    grid: &web_sys::Element,
    row: u16,
    column: u16,
) -> Result<Option<web_sys::Element>, JsValue> {
    let addressed = format!(
        "pre:nth-child({}) [data-terminal-column=\"{}\"]",
        row + 1,
        column
    );
    if let Some(cell) = grid.query_selector(&addressed)? {
        return Ok(Some(cell));
    }
    grid.query_selector(&format!(
        "pre:nth-child({}) > span:nth-child({})",
        row + 1,
        column + 1
    ))
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
        let cells = row.query_selector_all("span:not(.web-article-scroll-row)")?;
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
    browser_image_ids: Rc<RefCell<Vec<String>>>,
    scroll_settle_generation: Rc<Cell<u32>>,
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
    let scroll_viewport = Rc::clone(&viewport);
    let scroll_image_ids = Rc::clone(&browser_image_ids);
    let scroll_generation = Rc::clone(&scroll_settle_generation);
    let scroll_render_pending = Rc::new(Cell::new(false));
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
        let Ok(Some(row_height)) = browser_terminal_row_height(&grid) else {
            return;
        };
        let scroll_top = f64::from(scroll_terminal.scroll_top().max(0));
        let rendered_row = grid
            .get_attribute("data-rendered-article-scroll")
            .and_then(|row| row.parse::<u16>().ok())
            .unwrap_or_else(|| scroll_app.borrow().article_scroll());
        let _ = apply_browser_article_scroll_offset(&grid, scroll_top, rendered_row, row_height);
        let _ = position_browser_images(&document, scroll_top);

        if !scroll_render_pending.replace(true) {
            let render_pending = Rc::clone(&scroll_render_pending);
            let render_app = Rc::clone(&scroll_app);
            let render_terminal = scroll_terminal.clone();
            spawn_local(async move {
                // Keep the virtual Ratatui document close to native momentum scrolling
                // without rebuilding its DOM for every raw browser scroll event.
                TimeoutFuture::new(24).await;
                render_pending.set(false);
                if render_app.borrow().selected() != Tab::Articles
                    || render_app.borrow().opened_article().is_none()
                {
                    return;
                }
                let Some(document) = web_sys::window().and_then(|window| window.document()) else {
                    return;
                };
                let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
                    return;
                };
                let Ok(Some(row_height)) = browser_terminal_row_height(&grid) else {
                    return;
                };
                let row = native_scroll_row(
                    f64::from(render_terminal.scroll_top().max(0)),
                    row_height,
                    render_app.borrow().article_scroll_limit(),
                );
                if render_app.borrow().article_scroll() != row {
                    let _ = clear_cell_class(&grid, ".web-native-image-cell");
                    let _ = render_app
                        .borrow_mut()
                        .update(Message::SetArticleScroll(row));
                }
            });
        }

        let generation = scroll_generation.get().wrapping_add(1);
        scroll_generation.set(generation);
        let settle_generation = Rc::clone(&scroll_generation);
        let settle_app = Rc::clone(&scroll_app);
        let settle_viewport = Rc::clone(&scroll_viewport);
        let settle_image_ids = Rc::clone(&scroll_image_ids);
        let settle_terminal = scroll_terminal.clone();
        spawn_local(async move {
            TimeoutFuture::new(120).await;
            if settle_generation.get() != generation {
                return;
            }
            if settle_app.borrow().selected() != Tab::Articles
                || settle_app.borrow().opened_article().is_none()
            {
                return;
            }
            let Some(document) = web_sys::window().and_then(|window| window.document()) else {
                return;
            };
            let Some(grid) = document.get_element_by_id("terminal_ratzilla_grid") else {
                return;
            };
            let Ok(Some(row_height)) = browser_terminal_row_height(&grid) else {
                return;
            };
            let row = native_scroll_row(
                f64::from(settle_terminal.scroll_top().max(0)),
                row_height,
                settle_app.borrow().article_scroll_limit(),
            );
            if settle_app.borrow().article_scroll() != row {
                let _ = clear_cell_class(&grid, ".web-native-image-cell");
                let _ = settle_app
                    .borrow_mut()
                    .update(Message::SetArticleScroll(row));
            }
            let app = settle_app.borrow();
            let _ = sync_browser_images(&app, settle_viewport.get(), &settle_image_ids);
            let _ = sync_browser_text_selection();
        });
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

        if let Some(button) = controls.query_selector("[data-article-action=\"comment\"]")? {
            let button_app = Rc::clone(&app);
            let activate = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
                event.prevent_default();
                begin_comment(Rc::clone(&button_app));
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
        if event.key() == "Escape" && overlay_or_account_menu_open() {
            event.prevent_default();
            event.stop_propagation();
            close_all_overlays();
            return;
        }
        if modal_open() {
            return;
        }
        if event.meta_key() || event.ctrl_key() || event.alt_key() {
            return;
        }
        if let Some(code) = browser_key_code(&event.key()) {
            event.prevent_default();
            handle_key(Rc::clone(&key_app), code);
        }
    });
    window.add_event_listener_with_callback_and_bool(
        "keydown",
        keydown.as_ref().unchecked_ref(),
        true,
    )?;
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
        if article_open && char_is(&code, &['m', 'ь']) {
            begin_comment(app);
            return;
        }
        if article_open && char_is(&code, &['a', 'ф']) {
            show_auth_modal(false, false, app.borrow().language());
            return;
        }
        if article_open && char_is(&code, &['s', 'ы']) {
            show_auth_modal(false, true, app.borrow().language());
            return;
        }
        if article_open && char_is(&code, &['d', 'в']) && app.borrow().signed_in() {
            logout(app);
            return;
        }
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
    if let Some(target) = event
        .target()
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
    {
        let cell = if target.has_attribute("data-terminal-column") {
            Some(target)
        } else {
            target.closest("[data-terminal-column]").ok().flatten()
        };
        if let Some(cell) = cell
            && let Some(column) = cell
                .get_attribute("data-terminal-column")
                .and_then(|column| column.parse::<u16>().ok())
            && let Some(row) = cell.closest("pre[data-terminal-row]").ok().flatten()
            && let Some(row) = row
                .get_attribute("data-terminal-row")
                .and_then(|row| row.parse::<u16>().ok())
        {
            return (
                column.min(area.width.saturating_sub(1)),
                row.min(area.height.saturating_sub(1)),
            );
        }
    }
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
    let image = {
        let app = app.borrow();
        browser_image_at(area, column, row, &app)
    };
    if let Some((image_url, alt)) = image {
        show_image_viewer(&image_url, &alt);
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
    let comment_action = {
        let app = app.borrow();
        svetsec_ui::comment_action_at(area, column, row, &app)
    };
    if let Some(action) = comment_action {
        let language = app.borrow().language();
        match action {
            svetsec_ui::CommentAction::Login => show_auth_modal(false, false, language),
            svetsec_ui::CommentAction::Register => show_auth_modal(false, true, language),
            svetsec_ui::CommentAction::Add => begin_comment(Rc::clone(&app)),
            svetsec_ui::CommentAction::Logout => logout(Rc::clone(&app)),
        }
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

fn sync_browser_image_positions() -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let scroll_top = document
        .get_element_by_id("terminal")
        .map_or(0.0, |terminal| f64::from(terminal.scroll_top().max(0)));
    position_browser_images(&document, scroll_top)
}

fn sync_browser_images(
    app: &App,
    area: ratzilla::ratatui::layout::Rect,
    previous_ids: &RefCell<Vec<String>>,
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

    clear_cell_class(&grid, ".web-native-image-cell")?;
    let viewport = svetsec_ui::native_image_viewport(area, app);
    let article_open = app.selected() == Tab::Articles && app.opened_article().is_some();
    let scroll_top = if article_open {
        document
            .get_element_by_id("terminal")
            .map_or(0.0, |terminal| f64::from(terminal.scroll_top().max(0)))
    } else {
        0.0
    };
    let document_scroll_rows = if article_open {
        i32::from(app.article_scroll())
    } else {
        0
    };
    let placements = svetsec_ui::native_image_placements(area, app);
    let mut active_ids = Vec::with_capacity(placements.len());
    for placement in &placements {
        let id = browser_image_id(placement.key, placement.source);
        active_ids.push(id.clone());
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
        let document_top =
            row_rect.top() + f64::from(placement.y + document_scroll_rows) * row_height;
        let width = f64::from(placement.width) * cell_width;
        let height = f64::from(placement.height) * row_height;
        let Some(viewport) = viewport else {
            continue;
        };
        let content_top = row_rect.top() + f64::from(viewport.top()) * row_height;
        let content_right = row_rect.left() + f64::from(viewport.right()) * cell_width;
        let content_bottom = row_rect.top() + f64::from(viewport.bottom()) * row_height;
        let source = browser_image_url(placement.source);
        if image.get_attribute("src").as_deref() != Some(&source) {
            image.set_attribute("src", &source)?;
        }
        image.set_attribute("alt", placement.alt)?;
        image.set_attribute("data-document-top", &document_top.to_string())?;
        image.set_attribute("data-content-top", &content_top.to_string())?;
        image.set_attribute("data-content-right", &content_right.to_string())?;
        image.set_attribute("data-content-bottom", &content_bottom.to_string())?;
        image.set_attribute("data-image-left", &left.to_string())?;
        image.set_attribute("data-image-width", &width.to_string())?;
        image.set_attribute("data-image-height", &height.to_string())?;
        let Some(html_image) = image.dyn_ref::<web_sys::HtmlElement>() else {
            continue;
        };
        html_image
            .style()
            .set_property("left", &format!("{left}px"))?;
        html_image
            .style()
            .set_property("width", &format!("{width}px"))?;
        html_image
            .style()
            .set_property("height", &format!("{height}px"))?;
        html_image
            .style()
            .set_property("border-radius", if placement.rounded { "50%" } else { "0" })?;

        let visible_height = placement
            .height
            .saturating_sub(placement.clip_top)
            .saturating_sub(placement.clip_bottom);
        let visible_width = placement.width.saturating_sub(placement.clip_right);
        if visible_height > 0 && visible_width > 0 {
            set_area_class(
                &grid,
                ratzilla::ratatui::layout::Rect::new(
                    placement.x.max(0) as u16,
                    (placement.y + i32::from(placement.clip_top)).max(0) as u16,
                    visible_width,
                    visible_height,
                ),
                "web-native-image-cell",
            )?;
        }
    }
    position_browser_images(&document, scroll_top)?;
    for id in previous_ids.borrow().iter() {
        if !active_ids.contains(id)
            && let Some(image) = document.get_element_by_id(id)
        {
            image.remove();
        }
    }
    *previous_ids.borrow_mut() = active_ids;
    Ok(())
}

fn browser_image_at(
    area: ratzilla::ratatui::layout::Rect,
    column: u16,
    row: u16,
    app: &App,
) -> Option<(String, String)> {
    svetsec_ui::native_image_placements(area, app)
        .into_iter()
        .find(|placement| {
            let visible_height = placement
                .height
                .saturating_sub(placement.clip_top)
                .saturating_sub(placement.clip_bottom);
            let visible_width = placement.width.saturating_sub(placement.clip_right);
            let visible_area = ratzilla::ratatui::layout::Rect::new(
                placement.x.max(0) as u16,
                (placement.y + i32::from(placement.clip_top)).max(0) as u16,
                visible_width,
                visible_height,
            );
            !visible_area.is_empty() && visible_area.contains((column, row).into())
        })
        .map(|placement| {
            (
                browser_image_url(placement.source),
                placement.alt.to_owned(),
            )
        })
}

fn show_image_viewer(source: &str, alt: &str) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Some(viewer) = document.get_element_by_id("image-viewer") else {
        return;
    };
    close_all_overlays();
    if let Some(image) = document.get_element_by_id("image-viewer-image") {
        let _ = image.set_attribute("src", source);
        let _ = image.set_attribute("alt", alt);
    }
    if let Some(caption) = document.get_element_by_id("image-viewer-caption") {
        caption.set_text_content(Some(alt));
    }
    let _ = viewer.remove_attribute("hidden");
    if let Ok(Some(button)) = viewer.query_selector(".image-viewer-close")
        && let Some(button) = button.dyn_ref::<web_sys::HtmlElement>()
    {
        let _ = button.focus();
    }
}

fn browser_image_url(source: &str) -> String {
    if source.starts_with('/') {
        source.to_owned()
    } else {
        format!("/api/github/assets/{source}")
    }
}

fn browser_image_id(key: usize, source: &str) -> String {
    let hash = source
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    format!("article-native-image-{key}-{hash:016x}")
}

fn position_browser_images(document: &web_sys::Document, scroll_top: f64) -> Result<(), JsValue> {
    let images = document.query_selector_all(".native-article-image")?;
    for index in 0..images.length() {
        let Some(node) = images.item(index) else {
            continue;
        };
        let Some(image) = node.dyn_ref::<web_sys::HtmlElement>() else {
            continue;
        };
        let number = |name: &str| {
            image
                .get_attribute(name)
                .and_then(|value| value.parse::<f64>().ok())
        };
        let (Some(document_top), Some(content_top), Some(content_right), Some(content_bottom)) = (
            number("data-document-top"),
            number("data-content-top"),
            number("data-content-right"),
            number("data-content-bottom"),
        ) else {
            continue;
        };
        let (Some(left), Some(width), Some(height)) = (
            number("data-image-left"),
            number("data-image-width"),
            number("data-image-height"),
        ) else {
            continue;
        };
        let top = document_top - scroll_top;
        let clip_top = (content_top - top).clamp(0.0, height);
        let clip_right = (left + width - content_right).clamp(0.0, width);
        let clip_bottom = (top + height - content_bottom).clamp(0.0, height);
        let visible = clip_top + clip_bottom < height && clip_right < width;
        image
            .style()
            .set_property("display", if visible { "block" } else { "none" })?;
        image.style().set_property("top", &format!("{top}px"))?;
        image.style().set_property(
            "clip-path",
            &format!("inset({clip_top}px {clip_right}px {clip_bottom}px 0px)"),
        )?;
    }
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

fn load_session(app: Rc<RefCell<App>>) {
    spawn_local(async move {
        if let Ok(state) = fetch_session("GET", "/api/session", None).await {
            apply_session_state(&app, state);
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
    show_auth_modal(true, false, app.borrow().language());
}

struct SessionState {
    authenticated: bool,
    username: Option<String>,
    avatar_url: Option<String>,
    telegram_enabled: bool,
    can_moderate_comments: bool,
}

fn apply_session_state(app: &Rc<RefCell<App>>, state: SessionState) {
    let mut app = app.borrow_mut();
    let _ = app.update(Message::SetAuthenticated(state.authenticated));
    app.set_user(state.username);
    app.set_avatar_url(state.avatar_url);
    app.set_telegram_login_enabled(state.telegram_enabled);
    app.set_can_moderate_comments(state.can_moderate_comments);
}

fn install_account_events(app: Rc<RefCell<App>>) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    if let (Some(trigger), Some(menu)) = (
        document.get_element_by_id("account-trigger"),
        document.get_element_by_id("account-menu"),
    ) {
        let trigger_for_click = trigger.clone();
        let menu_for_click = menu.clone();
        let toggle = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
            event.prevent_default();
            let opening = menu_for_click.has_attribute("hidden");
            close_all_overlays();
            if opening {
                let _ = menu_for_click.remove_attribute("hidden");
            }
            let _ = trigger_for_click
                .set_attribute("aria-expanded", if opening { "true" } else { "false" });
            set_modal_error("account-error", "");
        });
        trigger.add_event_listener_with_callback("click", toggle.as_ref().unchecked_ref())?;
        toggle.forget();
    }

    if let Some(button) = document.get_element_by_id("account-logout") {
        let logout_app = Rc::clone(&app);
        let sign_out = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
            event.prevent_default();
            hide_account_menu();
            logout(Rc::clone(&logout_app));
        });
        button.add_event_listener_with_callback("click", sign_out.as_ref().unchecked_ref())?;
        sign_out.forget();
    }

    if let Some(input) = document
        .get_element_by_id("account-avatar-input")
        .and_then(|input| input.dyn_into::<HtmlInputElement>().ok())
    {
        let upload_app = Rc::clone(&app);
        let upload_input = input.clone();
        let change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            let Some(file) = upload_input.files().and_then(|files| files.get(0)) else {
                return;
            };
            set_modal_error("account-error", "Uploading…");
            let app = Rc::clone(&upload_app);
            spawn_local(async move {
                match upload_avatar_file(file).await {
                    Ok(state) => {
                        apply_session_state(&app, state);
                        set_modal_error("account-error", "Avatar updated.");
                    }
                    Err(error) => {
                        set_modal_error("account-error", &js_error_message(&error));
                    }
                }
            });
        });
        input.add_event_listener_with_callback("change", change.as_ref().unchecked_ref())?;
        change.forget();
    }

    for id in ["account-telegram-login", "telegram-login"] {
        let Some(link) = document.get_element_by_id(id) else {
            continue;
        };
        let error_id = if id == "telegram-login" {
            "auth-error"
        } else {
            "account-error"
        };
        let guard = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
            if event
                .current_target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                .is_some_and(|link| link.get_attribute("aria-disabled").as_deref() == Some("true"))
            {
                event.prevent_default();
                set_modal_error(
                    error_id,
                    "Telegram login has not been configured on the server yet.",
                );
            }
        });
        link.add_event_listener_with_callback("click", guard.as_ref().unchecked_ref())?;
        guard.forget();
    }
    let auth_form = document
        .get_element_by_id("auth-form")
        .ok_or_else(|| JsValue::from_str("auth form unavailable"))?;
    for action in ["login", "register"] {
        let Some(button) = auth_form.query_selector(&format!("[data-auth-action=\"{action}\"]"))?
        else {
            continue;
        };
        let form = auth_form.clone();
        let action = action.to_owned();
        let select = Closure::<dyn FnMut(MouseEvent)>::new(move |_: MouseEvent| {
            let _ = form.set_attribute("data-action", &action);
        });
        button.add_event_listener_with_callback("click", select.as_ref().unchecked_ref())?;
        select.forget();
    }

    let auth_app = Rc::clone(&app);
    let submit = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        event.prevent_default();
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let owner = document
            .get_element_by_id("auth-modal")
            .and_then(|modal| modal.get_attribute("data-mode"))
            .as_deref()
            == Some("owner");
        let register = document
            .get_element_by_id("auth-form")
            .and_then(|form| form.get_attribute("data-action"))
            .as_deref()
            == Some("register");
        let username = input_value(&document, "auth-username");
        let password = input_value(&document, "auth-password");
        if password.is_empty() || (!owner && username.is_empty()) {
            set_modal_error("auth-error", "Fill in all fields.");
            return;
        }
        set_modal_error("auth-error", "Working…");
        let app = Rc::clone(&auth_app);
        spawn_local(async move {
            let (url, body) = if owner {
                (
                    "/api/session",
                    serde_json::json!({ "password": password }).to_string(),
                )
            } else {
                (
                    if register {
                        "/api/users"
                    } else {
                        "/api/users/session"
                    },
                    serde_json::json!({ "username": username, "password": password }).to_string(),
                )
            };
            match fetch_session("POST", url, Some(body)).await {
                Ok(state) => {
                    apply_session_state(&app, state);
                    clear_input("auth-password");
                    hide_modal("auth-modal");
                    refresh_comment_modal(&app);
                }
                Err(error) => {
                    let language = app.borrow().language();
                    set_modal_error(
                        "auth-error",
                        &localized_account_error(&js_error_message(&error), language),
                    );
                }
            }
        });
    });
    auth_form.add_event_listener_with_callback("submit", submit.as_ref().unchecked_ref())?;
    submit.forget();

    let comment_form = document
        .get_element_by_id("comment-form")
        .ok_or_else(|| JsValue::from_str("comment form unavailable"))?;
    let comment_app = Rc::clone(&app);
    let submit = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        event.prevent_default();
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let body = textarea_value(&document, "comment-body");
        let slug = comment_app
            .borrow()
            .opened_article()
            .map(|article| article.slug.clone());
        let Some(slug) = slug else {
            hide_modal("comment-modal");
            return;
        };
        if body.trim().is_empty() {
            set_modal_error("comment-error", "Write a comment first.");
            return;
        }
        set_modal_error("comment-error", "Publishing…");
        let app = Rc::clone(&comment_app);
        spawn_local(async move {
            let payload = serde_json::json!({ "body": body }).to_string();
            match request_json(
                "POST",
                &format!("/api/articles/{slug}/comments"),
                Some(payload),
            )
            .await
            {
                Ok(_) => {
                    clear_textarea("comment-body");
                    hide_modal("comment-modal");
                    load_comments(app);
                }
                Err(error) => set_modal_error("comment-error", &js_error_message(&error)),
            }
        });
    });
    comment_form.add_event_listener_with_callback("submit", submit.as_ref().unchecked_ref())?;
    submit.forget();

    if let Some(button) = document.get_element_by_id("comment-sign-in") {
        let login_app = Rc::clone(&app);
        let open = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
            event.prevent_default();
            show_auth_modal(false, false, login_app.borrow().language());
        });
        button.add_event_listener_with_callback("click", open.as_ref().unchecked_ref())?;
        open.forget();
    }

    let close_buttons = document.query_selector_all("[data-modal-close]")?;
    for index in 0..close_buttons.length() {
        let Some(button) = close_buttons.item(index) else {
            continue;
        };
        let close = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
            event.prevent_default();
            close_all_overlays();
        });
        button.add_event_listener_with_callback("click", close.as_ref().unchecked_ref())?;
        close.forget();
    }
    for id in MODAL_IDS {
        let Some(overlay) = document.get_element_by_id(id) else {
            continue;
        };
        let id = id.to_owned();
        let close = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
            let clicked_backdrop = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                .is_some_and(|target| target.id() == id);
            if clicked_backdrop {
                event.prevent_default();
                close_all_overlays();
            }
        });
        overlay.add_event_listener_with_callback("click", close.as_ref().unchecked_ref())?;
        close.forget();
    }
    if document.get_element_by_id("web-account").is_some() {
        let close = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
            let clicked_account = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                .and_then(|target| target.closest("#web-account").ok().flatten())
                .is_some();
            if !clicked_account {
                hide_account_menu();
            }
        });
        document.add_event_listener_with_callback("click", close.as_ref().unchecked_ref())?;
        close.forget();
    }
    install_comment_delete_events(&document, app)?;
    Ok(())
}

fn install_comment_delete_events(
    document: &web_sys::Document,
    app: Rc<RefCell<App>>,
) -> Result<(), JsValue> {
    for id in ["web-comments-panel", "comment-list"] {
        let Some(container) = document.get_element_by_id(id) else {
            continue;
        };
        let delete_app = Rc::clone(&app);
        let click = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
            let comment_id = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                .and_then(|target| target.get_attribute("data-comment-delete"))
                .and_then(|value| value.parse::<i64>().ok());
            let Some(comment_id) = comment_id else {
                return;
            };
            event.prevent_default();
            event.stop_propagation();
            begin_delete_comment(Rc::clone(&delete_app), comment_id);
        });
        container.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
        click.forget();
    }
    Ok(())
}

fn show_auth_modal(owner: bool, register: bool, language: svetsec_core::Language) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Some(modal) = document.get_element_by_id("auth-modal") else {
        return;
    };
    close_all_overlays();
    let _ = modal.set_attribute("data-mode", if owner { "owner" } else { "reader" });
    let _ = document.get_element_by_id("auth-form").and_then(|form| {
        form.set_attribute("data-action", if register { "register" } else { "login" })
            .ok()
    });
    localize_account_modals(&document, language);
    if let Some(title) = document.get_element_by_id("auth-title") {
        title.set_text_content(Some(match (owner, register, language) {
            (true, _, svetsec_core::Language::En) => "Owner sign in",
            (true, _, svetsec_core::Language::Ru) => "Вход владельца",
            (false, true, svetsec_core::Language::En) => "Create reader account",
            (false, true, svetsec_core::Language::Ru) => "Аккаунт читателя",
            (false, false, svetsec_core::Language::En) => "Reader sign in",
            (false, false, svetsec_core::Language::Ru) => "Вход читателя",
        }));
    }
    if let Some(password) = document
        .get_element_by_id("auth-password")
        .and_then(|input| input.dyn_into::<HtmlInputElement>().ok())
    {
        password.set_autocomplete(if register {
            "new-password"
        } else {
            "current-password"
        });
    }
    if let Some(username) = document
        .get_element_by_id("auth-username")
        .and_then(|input| input.dyn_into::<HtmlInputElement>().ok())
    {
        username.set_disabled(owner);
        username.set_required(!owner);
    }
    set_modal_error("auth-error", "");
    let _ = modal.remove_attribute("hidden");
    if owner {
        if let Some(input) = document
            .get_element_by_id("auth-password")
            .and_then(|input| input.dyn_into::<HtmlInputElement>().ok())
        {
            let _ = input.focus();
        }
    } else if let Some(link) = document
        .get_element_by_id("telegram-login")
        .and_then(|link| link.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = link.focus();
    }
}

fn localize_account_modals(document: &web_sys::Document, language: svetsec_core::Language) {
    let texts = match language {
        svetsec_core::Language::En => [
            ("auth-username-label", "Username"),
            ("auth-password-label", "Password"),
            ("auth-login", "Sign in"),
            ("auth-register", "Register"),
            ("auth-cancel", "Cancel"),
            ("telegram-login", "Continue with Telegram"),
            (
                "telegram-login-help",
                "No separate site password is stored.",
            ),
            (
                "auth-requirements",
                "Username: 3–24 Latin letters, numbers, _ or -. Password: 8–128 characters. guest, owner, and svetsec are reserved.",
            ),
            ("comment-title", "Comments"),
            ("comment-sign-in", "Sign in / register"),
            ("comment-message-label", "Message"),
            ("comment-publish", "Publish"),
            ("comment-cancel", "Cancel"),
        ],
        svetsec_core::Language::Ru => [
            ("auth-username-label", "Имя пользователя"),
            ("auth-password-label", "Пароль"),
            ("auth-login", "Sign in"),
            ("auth-register", "Register"),
            ("auth-cancel", "Cancel"),
            ("telegram-login", "Continue with Telegram"),
            (
                "telegram-login-help",
                "Отдельный пароль сайта не сохраняется.",
            ),
            (
                "auth-requirements",
                "Имя: 3–24 латинских символа (A–Z, 0–9, _ или -). Пароль: 8–128 символов. guest, owner и svetsec зарезервированы.",
            ),
            ("comment-title", "Комментарии"),
            ("comment-sign-in", "Sign in / register"),
            ("comment-message-label", "Сообщение"),
            ("comment-publish", "Publish"),
            ("comment-cancel", "Cancel"),
        ],
    };
    for (id, text) in texts {
        if let Some(element) = document.get_element_by_id(id) {
            element.set_text_content(Some(text));
        }
    }
}

fn begin_comment(app: Rc<RefCell<App>>) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Some(modal) = document.get_element_by_id("comment-modal") else {
        return;
    };
    close_all_overlays();
    localize_account_modals(&document, app.borrow().language());
    populate_comment_modal(&document, &app.borrow());
    set_modal_error("comment-error", "");
    let _ = modal.remove_attribute("hidden");
    if let Some(input) = document
        .get_element_by_id("comment-body")
        .and_then(|input| input.dyn_into::<HtmlTextAreaElement>().ok())
    {
        let _ = input.focus();
    }
}

fn refresh_comment_modal(app: &Rc<RefCell<App>>) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    if document
        .get_element_by_id("comment-modal")
        .is_some_and(|modal| !modal.has_attribute("hidden"))
    {
        populate_comment_modal(&document, &app.borrow());
    }
}

fn populate_comment_modal(document: &web_sys::Document, app: &App) {
    let Some(modal) = document.get_element_by_id("comment-modal") else {
        return;
    };
    let _ = modal.set_attribute(
        "data-signed-in",
        if app.signed_in() { "true" } else { "false" },
    );
    let Some(list) = document.get_element_by_id("comment-list") else {
        return;
    };
    list.set_text_content(None);
    if app.comments_loading() || app.comments_error().is_some() {
        if let Ok(message) = document.create_element("p") {
            message.set_text_content(Some(if app.comments_loading() {
                match app.language() {
                    svetsec_core::Language::En => "Loading comments…",
                    svetsec_core::Language::Ru => "Загрузка комментариев…",
                }
            } else {
                app.comments_error().unwrap_or_default()
            }));
            let _ = list.append_child(&message);
        }
        return;
    }
    if app.comments().is_empty() {
        if let Ok(empty) = document.create_element("p") {
            empty.set_text_content(Some(match app.language() {
                svetsec_core::Language::En => "No comments yet.",
                svetsec_core::Language::Ru => "Комментариев пока нет.",
            }));
            let _ = list.append_child(&empty);
        }
        return;
    }
    for comment in app.comments() {
        let _ = append_comment_entry(document, &list, comment, app.can_moderate_comments());
    }
}

fn begin_delete_comment(app: Rc<RefCell<App>>, comment_id: i64) {
    let (slug, language, can_moderate) = {
        let app = app.borrow();
        (
            app.opened_article().map(|article| article.slug.clone()),
            app.language(),
            app.can_moderate_comments(),
        )
    };
    let Some(slug) = slug else {
        return;
    };
    if !can_moderate {
        return;
    }
    let question = match language {
        svetsec_core::Language::En => "Delete this comment permanently?",
        svetsec_core::Language::Ru => "Удалить этот комментарий безвозвратно?",
    };
    let confirmed = web_sys::window()
        .and_then(|window| window.confirm_with_message(question).ok())
        .unwrap_or(false);
    if !confirmed {
        return;
    }

    set_modal_error(
        "comment-error",
        match language {
            svetsec_core::Language::En => "Deleting…",
            svetsec_core::Language::Ru => "Удаление…",
        },
    );
    spawn_local(async move {
        let url = format!("/api/articles/{slug}/comments/{comment_id}");
        match request("DELETE", &url, None).await {
            Ok(_) => {
                set_modal_error("comment-error", "");
                load_comments(app);
            }
            Err(error) => {
                let message = js_error_message(&error);
                set_modal_error("comment-error", &message);
                if let Some(window) = web_sys::window() {
                    let _ = window.alert_with_message(&message);
                }
            }
        }
    });
}

fn logout(app: Rc<RefCell<App>>) {
    spawn_local(async move {
        if request("DELETE", "/api/session", None).await.is_ok() {
            apply_session_state(
                &app,
                SessionState {
                    authenticated: false,
                    username: None,
                    avatar_url: None,
                    telegram_enabled: app.borrow().telegram_login_enabled(),
                    can_moderate_comments: false,
                },
            );
            refresh_comment_modal(&app);
        }
    });
}

const MODAL_IDS: [&str; 3] = ["auth-modal", "comment-modal", "image-viewer"];

fn modal_open() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .is_some_and(|document| {
            MODAL_IDS.into_iter().any(|id| {
                document
                    .get_element_by_id(id)
                    .is_some_and(|modal| !modal.has_attribute("hidden"))
            })
        })
}

fn overlay_or_account_menu_open() -> bool {
    modal_open()
        || web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("account-menu"))
            .is_some_and(|menu| !menu.has_attribute("hidden"))
}

fn close_all_overlays() {
    for id in MODAL_IDS {
        hide_modal(id);
    }
    hide_account_menu();
}

fn hide_account_menu() {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    if let Some(menu) = document.get_element_by_id("account-menu") {
        let _ = menu.set_attribute("hidden", "");
    }
    if let Some(trigger) = document.get_element_by_id("account-trigger") {
        let _ = trigger.set_attribute("aria-expanded", "false");
    }
}

fn hide_modal(id: &str) {
    if let Some(modal) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
    {
        let _ = modal.set_attribute("hidden", "");
    }
}

fn input_value(document: &web_sys::Document, id: &str) -> String {
    document
        .get_element_by_id(id)
        .and_then(|input| input.dyn_into::<HtmlInputElement>().ok())
        .map_or_else(String::new, |input| input.value())
}

fn textarea_value(document: &web_sys::Document, id: &str) -> String {
    document
        .get_element_by_id(id)
        .and_then(|input| input.dyn_into::<HtmlTextAreaElement>().ok())
        .map_or_else(String::new, |input| input.value())
}

fn clear_input(id: &str) {
    if let Some(input) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
        .and_then(|input| input.dyn_into::<HtmlInputElement>().ok())
    {
        input.set_value("");
    }
}

fn clear_textarea(id: &str) {
    if let Some(input) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
        .and_then(|input| input.dyn_into::<HtmlTextAreaElement>().ok())
    {
        input.set_value("");
    }
}

fn set_modal_error(id: &str, message: &str) {
    if let Some(error) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
    {
        error.set_text_content(Some(message));
    }
}

fn js_error_message(error: &JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| "Request failed. Try again.".into())
}

fn localized_account_error(message: &str, language: svetsec_core::Language) -> String {
    if language == svetsec_core::Language::En {
        return message.to_owned();
    }
    match message {
        "username must use 3-24 Latin letters, numbers, _ or -" => {
            "Имя должно содержать 3–24 латинских символа: буквы, цифры, _ или -.".into()
        }
        "username is reserved" => "Это имя зарезервировано.".into(),
        "password must contain 8-128 characters" => {
            "Пароль должен содержать от 8 до 128 символов.".into()
        }
        "username is already registered" => "Это имя уже зарегистрировано.".into(),
        "invalid credentials" => "Неверное имя пользователя или пароль.".into(),
        _ => message.to_owned(),
    }
}

async fn fetch_session(
    method: &str,
    url: &str,
    body: Option<String>,
) -> Result<SessionState, JsValue> {
    let json = request_json(method, url, body).await?;
    session_state_from_json(&json)
}

fn session_state_from_json(json: &JsValue) -> Result<SessionState, JsValue> {
    let authenticated = js_sys::Reflect::get(json, &JsValue::from_str("authenticated"))?
        .as_bool()
        .unwrap_or(false);
    let username = js_sys::Reflect::get(json, &JsValue::from_str("username"))?.as_string();
    let avatar_url = js_sys::Reflect::get(json, &JsValue::from_str("avatar_url"))?.as_string();
    let telegram_enabled = js_sys::Reflect::get(json, &JsValue::from_str("telegram_enabled"))?
        .as_bool()
        .unwrap_or(false);
    let can_moderate_comments =
        js_sys::Reflect::get(json, &JsValue::from_str("can_moderate_comments"))?
            .as_bool()
            .unwrap_or(false);
    Ok(SessionState {
        authenticated,
        username,
        avatar_url,
        telegram_enabled,
        can_moderate_comments,
    })
}

async fn upload_avatar_file(file: web_sys::File) -> Result<SessionState, JsValue> {
    if file.size() <= 0.0 || file.size() > 3.0 * 1024.0 * 1024.0 {
        return Err(JsValue::from_str(
            "Choose a JPEG, PNG, or WebP image up to 3 MB.",
        ));
    }
    let content_type = file.type_();
    if !matches!(
        content_type.as_str(),
        "image/jpeg" | "image/png" | "image/webp"
    ) {
        return Err(JsValue::from_str("Choose a JPEG, PNG, or WebP image."));
    }
    let body = JsFuture::from(file.array_buffer()).await?;
    let options = RequestInit::new();
    options.set_method("POST");
    options.set_credentials(RequestCredentials::SameOrigin);
    options.set_body(&body);
    let request = Request::new_with_str_and_init("/api/users/avatar", &options)?;
    request.headers().set("Accept", "application/json")?;
    request.headers().set("Content-Type", &content_type)?;
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await?
        .dyn_into::<Response>()?;
    if !response.ok() {
        return Err(JsValue::from_str(&format!(
            "Avatar upload failed ({}).",
            response.status()
        )));
    }
    let json = JsFuture::from(response.json()?).await?;
    session_state_from_json(&json)
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
            Ok(article) => {
                app.borrow_mut().set_opened_article(article);
                load_comments(Rc::clone(&app));
            }
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

fn load_comments(app: Rc<RefCell<App>>) {
    let Some(slug) = app
        .borrow()
        .opened_article()
        .map(|article| article.slug.clone())
    else {
        return;
    };
    app.borrow_mut().begin_comments_load();
    spawn_local(async move {
        match fetch_comments(&slug).await {
            Ok(comments) => {
                app.borrow_mut().set_comments(comments);
                refresh_comment_modal(&app);
            }
            Err(_) => {
                let error = match app.borrow().language() {
                    svetsec_core::Language::En => "Could not load comments.",
                    svetsec_core::Language::Ru => "Не удалось загрузить комментарии.",
                };
                app.borrow_mut().set_comments_error(error);
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
            date: string("date"),
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

async fn fetch_comments(slug: &str) -> Result<Vec<Comment>, JsValue> {
    let json = request_json("GET", &format!("/api/articles/{slug}/comments"), None).await?;
    let mut comments = Vec::new();
    for value in js_sys::Array::from(&json).iter() {
        let string = |field: &str| {
            js_sys::Reflect::get(&value, &JsValue::from_str(field))
                .ok()
                .and_then(|value| value.as_string())
                .unwrap_or_default()
        };
        let number = |field: &str| {
            js_sys::Reflect::get(&value, &JsValue::from_str(field))
                .ok()
                .and_then(|value| value.as_f64())
                .unwrap_or_default() as i64
        };
        comments.push(Comment {
            id: number("id"),
            author: string("author"),
            owner: js_sys::Reflect::get(&value, &JsValue::from_str("owner"))?
                .as_bool()
                .unwrap_or(false),
            body: string("body"),
            created_at: number("created_at"),
        });
    }
    Ok(comments)
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
    let response = request(method, url, body).await?;
    JsFuture::from(response.json()?).await
}

async fn request(method: &str, url: &str, body: Option<String>) -> Result<Response, JsValue> {
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
        let status = response.status();
        let message = match response.json() {
            Ok(json) => JsFuture::from(json).await.ok().and_then(|json| {
                js_sys::Reflect::get(&json, &JsValue::from_str("error"))
                    .ok()
                    .and_then(|error| error.as_string())
            }),
            Err(_) => None,
        }
        .unwrap_or_else(|| format!("Request failed ({status})."));
        return Err(JsValue::from_str(&message));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use ratzilla::event::KeyCode;
    use ratzilla::ratatui::{Terminal, backend::TestBackend, widgets::Paragraph};
    use svetsec_core::{App, ArticleContent, ArticleImage, Message, Tab};

    use super::{
        DomSignature, RenderSignature, WebRoute, article_navigation_only_transition,
        article_scroll_offset, browser_image_at, browser_image_id, browser_key_code,
        cell_contains_selectable_text, grid_axis, grid_cell_axis, localized_account_error,
        native_scroll_row, rendered_scroll_top, selection_runs, structural_dom_transition,
        terminal_grid_size,
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
    fn phone_grid_uses_the_visible_terminal_instead_of_the_physical_screen() {
        assert_eq!(
            terminal_grid_size(390.0, 664.0, 10.0, 20.0),
            ratzilla::ratatui::layout::Size::new(39, 33)
        );
        assert_eq!(
            terminal_grid_size(844.0, 270.0, 10.0, 20.0),
            ratzilla::ratatui::layout::Size::new(84, 13)
        );

        let html = include_str!("../index.html");
        assert!(!html.contains("window.screen"));
        assert!(!html.contains("navigator.userAgent"));
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
    fn native_images_keep_stable_distinct_dom_ids() {
        let first = browser_image_id(0, "assets/first.jpg");
        assert_eq!(first, browser_image_id(0, "assets/first.jpg"));
        assert_ne!(first, browser_image_id(1, "assets/first.jpg"));
        assert_ne!(first, browser_image_id(0, "assets/second.jpg"));
    }

    #[test]
    fn visible_article_images_are_openable_at_their_rendered_cells() {
        let area = ratzilla::ratatui::layout::Rect::new(0, 0, 100, 40);
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.set_opened_article(ArticleContent {
            slug: "screenshot".into(),
            title: "Screenshot".into(),
            markdown: "![Screenshot](assets/screenshot.png)".into(),
            images: vec![ArticleImage {
                source: "assets/screenshot.png".into(),
                alt: "Screenshot".into(),
                width: 12,
                height: 6,
                pixels: Vec::new(),
            }],
            labels: Vec::new(),
        });
        let placement = svetsec_ui::native_image_placements(area, &app)[0];
        let column = placement.x.max(0) as u16;
        let row = (placement.y + i32::from(placement.clip_top)).max(0) as u16;

        assert_eq!(
            browser_image_at(area, column, row, &app),
            Some((
                "/api/github/assets/assets/screenshot.png".into(),
                "Screenshot".into()
            ))
        );
        assert_eq!(browser_image_at(area, 99, 39, &app), None);
    }

    #[test]
    fn full_redraw_restores_static_cells_after_backend_grid_reset() {
        let mut terminal = Terminal::new(TestBackend::new(12, 1)).unwrap();
        let render = |frame: &mut ratzilla::ratatui::Frame<'_>| {
            frame.render_widget(Paragraph::new("STATIC"), frame.area());
        };

        terminal.draw(render).unwrap();
        assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "S");

        // Recreate a same-sized, blank backend surface. Ratatui does not notice a
        // size change and therefore sends no unchanged text on the following draw.
        terminal.backend_mut().resize(0, 0);
        terminal.backend_mut().resize(12, 1);
        terminal.draw(render).unwrap();
        assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), " ");

        terminal.clear().unwrap();
        terminal.draw(render).unwrap();
        assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "S");
    }

    #[test]
    fn view_changes_clear_stale_dom_decorations_before_the_next_paint() {
        let area = ratzilla::ratatui::layout::Rect::new(0, 0, 100, 30);
        let mut app = App::default();
        let main = DomSignature::new(area, &app);

        let _ = app.update(Message::SelectTab(Tab::Projects));
        let projects = DomSignature::new(area, &app);
        assert!(structural_dom_transition(&main, &projects));

        let _ = app.update(Message::Hover(Some(svetsec_core::HelpTarget::Logo)));
        let hovered = DomSignature::new(area, &app);
        assert!(!structural_dom_transition(&projects, &hovered));

        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.begin_article_load();
        let loading = DomSignature::new(area, &app);
        assert!(structural_dom_transition(&hovered, &loading));
    }

    #[test]
    fn native_scroll_interpolates_between_terminal_rows() {
        let html = include_str!("../index.html");
        assert!(html.contains("position: sticky"));
        assert!(html.contains("--article-scroll-offset"));
        assert!(html.contains("translate3d"));
        assert!(html.contains("overflow-anchor: none"));
        assert!(html.contains("touch-action: pan-y pinch-zoom"));
        assert!(html.contains("-webkit-overflow-scrolling: touch"));
        assert!(html.contains("id=\"web-comments-panel\""));
        assert!(html.contains("overscroll-behavior: contain"));
        assert!(html.contains("id=\"image-viewer\""));
        assert!(html.contains("id=\"web-account\""));
        assert!(html.contains("aria-labelledby=\"auth-title\""));
        assert!(html.contains("aria-labelledby=\"comment-title\""));
        assert!(!html.contains("id=\"account-owner-login\""));
        assert!(html.contains(".comment-delete:hover"));
        assert!(html.contains(">Up</button>"));
        assert!(html.contains(">Down</button>"));
        assert!(!html.contains(">K ↑</button>"));
        assert!(!html.contains(">J ↓</button>"));
        assert_eq!(rendered_scroll_top(3, 19.5), 58.5);
        assert_eq!(native_scroll_row(58.4, 19.5, 20), 2);
        assert_eq!(native_scroll_row(58.5, 19.5, 20), 3);
        assert_eq!(native_scroll_row(999.0, 19.5, 4), 4);
        assert_eq!(article_scroll_offset(66.0, 3, 20.0), 6.0);
        assert_eq!(article_scroll_offset(39.0, 2, 20.0), -1.0);
        assert_eq!(article_scroll_offset(200.0, 2, 20.0), 20.0);
    }

    #[test]
    fn article_navigation_uses_the_lightweight_dom_sync_path() {
        let area = ratzilla::ratatui::layout::Rect::new(0, 0, 100, 30);
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.set_opened_article(ArticleContent {
            slug: "long-read".into(),
            title: "Long read".into(),
            markdown: (0..80).map(|row| format!("Row {row}\n")).collect(),
            images: Vec::new(),
            labels: Vec::new(),
        });
        app.set_article_viewport_rows(10);
        let before = DomSignature::new(area, &app);
        let _ = app.update(Message::SetArticleScroll(1));
        let after = DomSignature::new(area, &app);
        assert!(article_navigation_only_transition(&before, &after));

        let _ = app.update(Message::Hover(Some(svetsec_core::HelpTarget::Logo)));
        let hovered = DomSignature::new(area, &app);
        assert!(!article_navigation_only_transition(&after, &hovered));
    }

    #[test]
    fn render_signature_skips_idle_frames_but_tracks_animation() {
        let area = ratzilla::ratatui::layout::Rect::new(0, 0, 100, 30);
        let mut app = App::default();
        app.begin_articles_load();
        let before = RenderSignature::new(area, &app);
        let _ = app.update(Message::AdvanceSkeleton);
        let animated = RenderSignature::new(area, &app);
        assert_ne!(before, animated);
        assert_eq!(animated, RenderSignature::new(area, &app));
    }

    #[test]
    fn reader_account_errors_explain_registration_requirements() {
        assert_eq!(
            localized_account_error("username is reserved", svetsec_core::Language::Ru),
            "Это имя зарезервировано."
        );
        assert_eq!(
            localized_account_error(
                "password must contain 8-128 characters",
                svetsec_core::Language::Ru
            ),
            "Пароль должен содержать от 8 до 128 символов."
        );
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
