use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap},
};
use svetsec_core::{
    App, ArticleImage, DOT_WELL_FRAMES, DOT_WELL_LANGUAGE, DOT_WELL_ROWS, HelpTarget, Language, Tab,
};

const WHITE: Color = Color::Rgb(247, 251, 255);
const CANVAS: Color = Color::Rgb(7, 10, 14);
const PAPER: Color = Color::Rgb(10, 15, 21);
const SOFT_GRAY: Color = Color::Rgb(28, 42, 54);
const PANEL: Color = Color::Rgb(13, 19, 27);
const PANEL_ALT: Color = Color::Rgb(17, 25, 35);
const INK: Color = Color::Rgb(230, 237, 243);
const GRAPHITE: Color = Color::Rgb(168, 179, 191);
const MID_GRAY: Color = Color::Rgb(82, 97, 112);
const BODY: Color = Color::Rgb(199, 208, 217);
const MUTED: Color = Color::Rgb(131, 145, 160);
const CONTROL: Color = Color::Rgb(37, 52, 66);
const CONTROL_ACTIVE: Color = Color::Rgb(49, 93, 130);
const ONLINE_GREEN: Color = Color::Rgb(54, 211, 126);
const CODE_KEYWORD: Color = Color::Rgb(187, 154, 247);
const CODE_STRING: Color = Color::Rgb(105, 203, 190);
const CODE_NUMBER: Color = Color::Rgb(244, 162, 97);
const CODE_COMMENT: Color = Color::Rgb(126, 142, 156);
const CODE_FOCUS: Color = Color::Rgb(24, 36, 51);
const MOBILE_BREAKPOINT: u16 = 56;

#[must_use]
pub fn label_color(label: &str) -> Color {
    match label.trim().to_lowercase().as_str() {
        "cryptography" | "crypto" | "криптография" | "крипто" => {
            Color::Rgb(102, 72, 190)
        }
        "security" => Color::Rgb(176, 52, 76),
        "rust" => Color::Rgb(174, 72, 24),
        "python" => Color::Rgb(44, 92, 156),
        "systems" => Color::Rgb(34, 116, 108),
        normalized => {
            const PALETTE: [Color; 8] = [
                Color::Rgb(88, 80, 156),
                Color::Rgb(144, 66, 122),
                Color::Rgb(54, 104, 150),
                Color::Rgb(132, 78, 42),
                Color::Rgb(48, 118, 92),
                Color::Rgb(116, 82, 146),
                Color::Rgb(154, 64, 70),
                Color::Rgb(70, 96, 118),
            ];
            let hash = normalized
                .bytes()
                .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                    (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
                });
            PALETTE[hash as usize % PALETTE.len()]
        }
    }
}

struct UiLayout {
    header: Rect,
    content: Rect,
    footer: Rect,
    logo: Rect,
    tabs: [Rect; 3],
    status: Option<Rect>,
    compact: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArticleImagePlacement<'a> {
    pub source: &'a str,
    pub alt: &'a str,
    pub x: i32,
    pub y: i32,
    pub width: u16,
    pub height: u16,
    pub clip_top: u16,
    pub clip_right: u16,
    pub clip_bottom: u16,
    pub rounded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeBlockAction {
    Run { block: usize, row: u16 },
    Copy { block: usize, row: u16 },
}

impl CodeBlockAction {
    #[must_use]
    pub const fn block(self) -> usize {
        match self {
            Self::Run { block, .. } | Self::Copy { block, .. } => block,
        }
    }

    #[must_use]
    pub const fn row(self) -> u16 {
        match self {
            Self::Run { row, .. } | Self::Copy { row, .. } => row,
        }
    }
}

#[must_use]
pub fn tab_at(area: Rect, column: u16, row: u16) -> Option<Tab> {
    tab_areas(area)
        .into_iter()
        .find_map(|(tab, area)| area.contains((column, row).into()).then_some(tab))
}

#[must_use]
pub fn tab_areas(area: Rect) -> [(Tab, Rect); 3] {
    let tabs = layout(area).tabs;
    [
        (Tab::Main, tabs[0]),
        (Tab::Articles, tabs[1]),
        (Tab::Info, tabs[2]),
    ]
}

#[must_use]
pub fn article_at(area: Rect, column: u16, row: u16, app: &App) -> Option<usize> {
    if app.selected() != Tab::Articles
        || app.articles_loading()
        || app.article_loading()
        || app.opened_article().is_some()
    {
        return None;
    }
    let content = layout(area).content;
    let primary = if content.width >= 76 {
        Layout::horizontal([
            Constraint::Percentage(68),
            Constraint::Length(1),
            Constraint::Percentage(32),
        ])
        .split(content)[0]
    } else {
        content
    };
    if column <= primary.left() || column >= primary.right().saturating_sub(1) {
        return None;
    }
    let first_row = primary.top().saturating_add(4);
    let index = row.checked_sub(first_row)? as usize;
    (index < app.articles().len().min(12)).then_some(index)
}

#[must_use]
pub fn article_areas(area: Rect, app: &App) -> Vec<(usize, Rect)> {
    if app.selected() != Tab::Articles
        || app.articles_loading()
        || app.article_loading()
        || app.opened_article().is_some()
    {
        return Vec::new();
    }
    let content = layout(area).content;
    let primary = if content.width >= 76 {
        Layout::horizontal([
            Constraint::Percentage(68),
            Constraint::Length(1),
            Constraint::Percentage(32),
        ])
        .split(content)[0]
    } else {
        content
    };
    let first_row = primary.top().saturating_add(4);
    (0..app.articles().len().min(12))
        .map(|index| {
            (
                index,
                Rect::new(
                    primary.left().saturating_add(1),
                    first_row.saturating_add(index as u16),
                    primary.width.saturating_sub(2),
                    1,
                ),
            )
        })
        .collect()
}

#[must_use]
pub fn article_back_area(area: Rect, app: &App) -> Option<Rect> {
    (app.selected() == Tab::Articles && app.opened_article().is_some()).then(|| {
        let (panel, compact) = primary_panel_area(area);
        let horizontal_padding = if compact { 1 } else { 2 };
        Rect::new(
            panel.left().saturating_add(1 + horizontal_padding),
            panel.top().saturating_add(2),
            10.min(panel.width.saturating_sub(2 + horizontal_padding * 2)),
            1,
        )
    })
}

#[must_use]
pub fn resume_link_area(area: Rect, app: &App) -> Option<Rect> {
    (app.selected() == Tab::Info).then(|| {
        let (panel, compact) = primary_panel_area(area);
        let horizontal_padding = if compact { 1 } else { 2 };
        Rect::new(
            panel.left().saturating_add(1 + horizontal_padding),
            panel.top().saturating_add(6),
            panel.width.saturating_sub(2 + horizontal_padding * 2),
            1,
        )
    })
}

#[must_use]
pub fn article_viewport_rows(area: Rect) -> u16 {
    let (_, _, _, _, height) = article_geometry(area);
    height.max(1)
}

#[must_use]
pub fn article_cursor_at(area: Rect, column: u16, row: u16, app: &App) -> Option<u16> {
    article_position_at(area, column, row, app).map(|(row, _)| row)
}

#[must_use]
pub fn article_position_at(area: Rect, column: u16, row: u16, app: &App) -> Option<(u16, u16)> {
    if app.selected() != Tab::Articles || app.opened_article().is_none() {
        return None;
    }
    let (_, _, left, top, height) = article_geometry(area);
    let right = left.saturating_add(article_content_width(area));
    if column < left || column >= right || row < top || row >= top.saturating_add(height) {
        return None;
    }
    let document_row = app.article_scroll().saturating_add(row.saturating_sub(top));
    let document_column = column.saturating_sub(left);
    (document_row < app.article_total_rows()).then_some((document_row, document_column))
}

#[must_use]
pub fn code_action_areas(area: Rect, app: &App) -> Vec<(CodeBlockAction, Rect)> {
    if app.selected() != Tab::Articles || app.opened_article().is_none() {
        return Vec::new();
    }
    let (_, _, left, top, height) = article_geometry(area);
    let right = left.saturating_add(article_content_width(area));
    let block_width = article_content_width(area).saturating_sub(2);
    let mut actions = Vec::new();
    let mut block_index = 0;
    while let Some(block) = app.article_code_block(block_index) {
        block_index += 1;
        if block.animated() {
            continue;
        }
        let screen_row =
            i32::from(top) + i32::from(block.start_row) - i32::from(app.article_scroll());
        if screen_row < i32::from(top) || screen_row >= i32::from(top.saturating_add(height)) {
            continue;
        }
        let y = screen_row as u16;
        let (run_offset, copy_offset) = code_action_offsets(block.executable(), block_width);
        if let Some(offset) = run_offset {
            let x = left.saturating_add(offset);
            let width = 5.min(right.saturating_sub(x));
            actions.push((
                CodeBlockAction::Run {
                    block: block.index,
                    row: block.start_row,
                },
                Rect::new(x, y, width, 1),
            ));
        }
        if let Some(offset) = copy_offset {
            let x = left.saturating_add(offset);
            actions.push((
                CodeBlockAction::Copy {
                    block: block.index,
                    row: block.start_row,
                },
                Rect::new(x, y, 6.min(right - x), 1),
            ));
        }
    }
    actions
}

fn code_action_offsets(executable: bool, width: u16) -> (Option<u16>, Option<u16>) {
    if executable && width >= 14 {
        (Some(width - 14), Some(width - 8))
    } else if width >= 8 {
        (None, Some(width - 8))
    } else {
        (None, None)
    }
}

#[must_use]
pub fn code_action_at(area: Rect, column: u16, row: u16, app: &App) -> Option<CodeBlockAction> {
    code_action_areas(area, app)
        .into_iter()
        .find_map(|(action, area)| area.contains((column, row).into()).then_some(action))
}

fn article_geometry(area: Rect) -> (Rect, bool, u16, u16, u16) {
    let (panel, compact) = primary_panel_area(area);
    let horizontal_padding = if compact { 1 } else { 2 };
    let bottom_padding = if compact { 0 } else { 1 };
    let left = panel.left().saturating_add(1 + horizontal_padding);
    let top = panel.top().saturating_add(2);
    let bottom = panel
        .bottom()
        .saturating_sub(1)
        .saturating_sub(bottom_padding);
    (panel, compact, left, top, bottom.saturating_sub(top))
}

fn primary_panel_area(area: Rect) -> (Rect, bool) {
    let content = layout(area).content;
    let compact = content.width < 76;
    let panel = if compact {
        content
    } else {
        Layout::horizontal([
            Constraint::Percentage(68),
            Constraint::Length(1),
            Constraint::Percentage(32),
        ])
        .split(content)[0]
    };
    (panel, compact)
}

fn article_content_width(area: Rect) -> u16 {
    let (panel, compact, left, _, _) = article_geometry(area);
    let horizontal_padding = if compact { 1 } else { 2 };
    panel
        .right()
        .saturating_sub(1 + horizontal_padding)
        .saturating_sub(left)
}

#[must_use]
pub fn help_target_at(area: Rect, column: u16, row: u16, app: &App) -> Option<HelpTarget> {
    let layout = layout(area);
    let position = (column, row).into();
    if python_output_close_area(area, app).is_some_and(|area| area.contains(position)) {
        return Some(HelpTarget::PythonOutputClose);
    }
    if let Some(action) = code_action_at(area, column, row, app) {
        return Some(match action {
            CodeBlockAction::Run { block, .. } => HelpTarget::CodeRun(block),
            CodeBlockAction::Copy { block, .. } => HelpTarget::CodeCopy(block),
        });
    }
    if let Some(index) = article_at(area, column, row, app) {
        return Some(HelpTarget::Article(index));
    }
    if article_back_area(area, app).is_some_and(|area| area.contains(position)) {
        return Some(HelpTarget::ArticleBack);
    }
    if resume_link_area(area, app).is_some_and(|area| area.contains(position)) {
        return Some(HelpTarget::Resume);
    }
    if layout.logo.contains(position) {
        return Some(HelpTarget::Logo);
    }
    if let Some((tab, _)) = tab_areas(area)
        .into_iter()
        .find(|(_, area)| area.contains(position))
    {
        return Some(HelpTarget::Tab(tab));
    }
    if layout.status.is_some_and(|area| area.contains(position)) {
        return Some(HelpTarget::Status);
    }
    (layout.content.contains(position)).then_some(HelpTarget::Articles)
}

#[must_use]
pub fn python_output_area(area: Rect, app: &App) -> Option<Rect> {
    if app.selected() != Tab::Articles
        || app.opened_article().is_none()
        || (!app.python_running() && app.python_output().is_none())
    {
        return None;
    }
    let content = layout(area).content;
    if content.width >= 76 {
        Some(
            Layout::horizontal([
                Constraint::Percentage(68),
                Constraint::Length(1),
                Constraint::Percentage(32),
            ])
            .split(content)[2],
        )
    } else {
        let height = content.height.clamp(4, 10);
        Some(Rect::new(
            content.left(),
            content.bottom().saturating_sub(height),
            content.width,
            height,
        ))
    }
}

#[must_use]
pub fn python_output_close_area(area: Rect, app: &App) -> Option<Rect> {
    let output = python_output_area(area, app)?;
    app.python_output().map(|_| {
        Rect::new(
            output.right().saturating_sub(5),
            output.top(),
            3.min(output.width),
            1,
        )
    })
}

#[must_use]
pub fn native_image_placements<'a>(area: Rect, app: &'a App) -> Vec<ArticleImagePlacement<'a>> {
    if app.language_notice() {
        return Vec::new();
    }
    let (primary, compact, _, _, _) = article_geometry(area);
    let horizontal_padding = if compact { 1 } else { 2 };
    let bottom_padding = if compact { 0 } else { 1 };
    let content_left = i32::from(primary.left()) + 1 + horizontal_padding;
    let content_right = i32::from(primary.right()) - 1 - horizontal_padding;
    let content_top = i32::from(primary.top()) + 2;
    let content_bottom = i32::from(primary.bottom()) - 1 - bottom_padding;
    let bounds = (content_right, content_top, content_bottom);

    match app.selected() {
        Tab::Info => app
            .profile_image()
            .and_then(|image| image_placement(image, content_left, content_top + 8, bounds, true))
            .into_iter()
            .collect(),
        Tab::Articles => {
            let Some(article) = app.opened_article() else {
                return Vec::new();
            };
            let metadata_rows = i32::from(!article.labels.is_empty());
            let markdown_top = content_top + 4 + metadata_rows - i32::from(app.article_scroll());
            markdown_image_offsets(&article.markdown, &article.images)
                .into_iter()
                .filter_map(|(image, offset)| {
                    image_placement(
                        image,
                        content_left,
                        markdown_top + i32::from(offset),
                        bounds,
                        false,
                    )
                })
                .collect()
        }
        Tab::Main => Vec::new(),
    }
}

fn image_placement<'a>(
    image: &'a ArticleImage,
    x: i32,
    y: i32,
    (content_right, content_top, content_bottom): (i32, i32, i32),
    rounded: bool,
) -> Option<ArticleImagePlacement<'a>> {
    let width = image.width;
    let height = image.height.div_ceil(2);
    let right = x + i32::from(width);
    let bottom = y + i32::from(height);
    let clip_top = (content_top - y).max(0).min(i32::from(height)) as u16;
    let clip_right = (right - content_right).max(0).min(i32::from(width)) as u16;
    let clip_bottom = (bottom - content_bottom).max(0).min(i32::from(height)) as u16;
    (clip_top + clip_bottom < height && clip_right < width).then_some(ArticleImagePlacement {
        source: &image.source,
        alt: &image.alt,
        x,
        y,
        width,
        height,
        clip_top,
        clip_right,
        clip_bottom,
        rounded,
    })
}

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(Block::new().style(Style::new().bg(CANVAS)), area);

    let layout = layout(area);
    render_header(frame, &layout, app);
    render_content(frame, layout.content, app);
    render_footer(frame, layout.footer, app, layout.compact);
    if app.language_notice() {
        render_language_notice(frame, area, app.language());
    }
}

fn render_header(frame: &mut Frame<'_>, layout: &UiLayout, app: &App) {
    paint_horizontal_background(frame.buffer_mut(), layout.header, PAPER, SOFT_GRAY);
    frame.render_widget(
        Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded),
        layout.header,
    );
    paint_gradient_border(frame.buffer_mut(), layout.header, INK, MID_GRAY);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "● ",
                Style::new().fg(if app.owner_online() {
                    ONLINE_GREEN
                } else {
                    MID_GRAY
                }),
            ),
            Span::styled(
                "svetsec.ru",
                Style::new().fg(INK).add_modifier(Modifier::BOLD),
            ),
        ]))
        .alignment(if layout.compact {
            Alignment::Center
        } else {
            Alignment::Left
        }),
        layout.logo,
    );

    for (tab, tab_area) in Tab::ALL.into_iter().zip(layout.tabs) {
        let selected_style = Style::new().fg(WHITE).add_modifier(Modifier::BOLD);
        let idle_style = Style::new().fg(MUTED);
        let hover_style = Style::new().fg(INK).add_modifier(Modifier::BOLD);
        let hovered = app.hovered() == Some(HelpTarget::Tab(tab));

        match (tab == app.selected(), hovered) {
            (true, true) => {
                paint_horizontal_background(frame.buffer_mut(), tab_area, CONTROL_ACTIVE, CONTROL);
            }
            (true, false) => {
                paint_horizontal_background(frame.buffer_mut(), tab_area, CONTROL_ACTIVE, CONTROL);
            }
            (false, true) => {
                paint_horizontal_background(frame.buffer_mut(), tab_area, SOFT_GRAY, PANEL_ALT);
            }
            (false, false) => {}
        }

        frame.render_widget(
            Paragraph::new(tab.label(app.language()))
                .alignment(Alignment::Center)
                .style(if tab == app.selected() {
                    selected_style
                } else if hovered {
                    hover_style
                } else {
                    idle_style
                }),
            tab_area,
        );
    }
}

fn render_language_notice(frame: &mut Frame<'_>, area: Rect, language: Language) {
    let width = 12.min(area.width);
    let height = 5.min(area.height);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::default(),
            Line::from(Span::styled(
                language.flag(),
                Style::new().fg(INK).add_modifier(Modifier::BOLD),
            ))
            .centered(),
        ])
        .block(
            Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(Style::new().bg(PAPER)),
        ),
        popup,
    );
    paint_gradient_border(frame.buffer_mut(), popup, INK, MID_GRAY);
}

fn render_content(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let show_python_output = app.selected() == Tab::Articles
        && app.opened_article().is_some()
        && (app.python_running() || app.python_output().is_some());
    if area.width >= 76 {
        let columns = Layout::horizontal([
            Constraint::Percentage(68),
            Constraint::Length(1),
            Constraint::Percentage(32),
        ])
        .split(area);
        render_primary_panel(frame, columns[0], app, false);
        if show_python_output {
            render_python_output_panel(frame, columns[2], app);
        } else {
            render_status_panel(frame, columns[2], app);
        }
    } else {
        render_primary_panel(frame, area, app, true);
        if show_python_output {
            let height = area.height.clamp(4, 10);
            let output_area = Rect::new(
                area.left(),
                area.bottom().saturating_sub(height),
                area.width,
                height,
            );
            render_python_output_panel(frame, output_area, app);
        }
    }
}

fn render_primary_panel(frame: &mut Frame<'_>, area: Rect, app: &App, compact: bool) {
    if app.selected() == Tab::Articles {
        render_articles_panel(frame, area, app, compact);
        return;
    }
    frame.render_widget(Block::new().style(Style::new().bg(PANEL)), area);

    let mut content = vec![
        Line::from(vec![
            Span::styled("● ONLINE", Style::new().fg(INK).bold()),
            Span::styled("  //  RUST + WASM", Style::new().fg(MUTED)),
        ]),
        Line::default(),
        Line::from(Span::styled(
            app.selected().title(app.language()),
            Style::new().fg(INK).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];
    if app.selected() == Tab::Info {
        content.push(Line::from(vec![
            Span::styled(
                match app.language() {
                    Language::En => "Resume PDF  ",
                    Language::Ru => "Резюме PDF  ",
                },
                Style::new().fg(INK).bold(),
            ),
            Span::styled(
                "https://svetsec.ru/assets/resume.pdf",
                Style::new().fg(Color::Rgb(143, 199, 242)).underlined(),
            ),
        ]));
        content.push(Line::default());
        content.push(Line::from(Span::styled(
            app.selected().description(app.language()),
            Style::new().fg(BODY),
        )));
        content.push(Line::default());
        if let Some(image) = app.profile_image() {
            content.extend(image_lines(image));
        }
    } else {
        content.push(Line::from(Span::styled(
            app.selected().description(app.language()),
            Style::new().fg(BODY),
        )));
        content.push(Line::default());
        content.push(Line::from(vec![
            Span::styled("SIGNAL  ", Style::new().fg(MUTED)),
            Span::styled("▁▂▃▅▇█▇▆▄▃▅▆", Style::new().fg(GRAPHITE)),
        ]));
    }

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {} ", app.selected().label(app.language()))).fg(INK))
        .padding(if compact {
            Padding::new(1, 1, 1, 0)
        } else {
            Padding::new(2, 2, 1, 1)
        });

    frame.render_widget(
        Paragraph::new(Text::from(content))
            .block(block)
            .wrap(Wrap { trim: true }),
        area,
    );
    paint_gradient_border(frame.buffer_mut(), area, INK, MID_GRAY);
}

fn render_articles_panel(frame: &mut Frame<'_>, area: Rect, app: &App, compact: bool) {
    frame.render_widget(Block::new().style(Style::new().bg(PANEL)), area);
    let mut lines = vec![if app.opened_article().is_some() {
        Line::from(vec![
            Span::styled(
                match (
                    app.language(),
                    app.hovered() == Some(HelpTarget::ArticleBack),
                ) {
                    (Language::En, false) => "← Back",
                    (Language::En, true) => "← BACK",
                    (Language::Ru, false) => "← Назад",
                    (Language::Ru, true) => "← НАЗАД",
                },
                Style::new()
                    .fg(Color::Rgb(143, 199, 242))
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
            Span::styled(
                "   ● SYNC  //  GITHUB main/articles",
                Style::new().fg(MUTED),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("● SYNC", Style::new().fg(INK).bold()),
            Span::styled("  //  GITHUB main/articles", Style::new().fg(MUTED)),
        ])
    }];

    if app.articles_loading() || app.article_loading() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            match app.language() {
                Language::En if app.article_loading() => "Loading Markdown…",
                Language::Ru if app.article_loading() => "Загружаем Markdown…",
                Language::En => "Loading article names…",
                Language::Ru => "Загружаем названия статей…",
            },
            Style::new().fg(MUTED),
        )));
        let loading_width = area.width.saturating_sub(if compact { 4 } else { 6 });
        if app.article_loading() {
            lines.extend(dot_well_lines(
                loading_width,
                app.article_animation_phase(),
                false,
                3,
                1.35,
            ));
        } else {
            for row in 0..5 {
                lines.push(skeleton_line(
                    area.width.saturating_sub(if compact { 6 } else { 10 }),
                    row,
                    app.skeleton_phase(),
                ));
            }
        }
    } else if let Some(article) = app.opened_article() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            article.title.clone(),
            Style::new().fg(INK).bold(),
        )));
        if !article.labels.is_empty() {
            lines.push(Line::from(label_spans(&article.labels)));
        }
        lines.push(Line::default());
        let focused_block = app.focused_code_block().map(|block| block.index);
        let article_width = area.width.saturating_sub(if compact { 4 } else { 6 });
        let code_block_width = article_width.saturating_sub(2);
        lines.extend(markdown_lines_with_focus(
            &article.markdown,
            &article.images,
            focused_block,
            code_block_width,
            app.article_animation_phase(),
            app.hovered(),
        ));
        lines.push(Line::default());
        let controls = match (app.language(), app.focused_code_block()) {
            (Language::En, Some(block)) if block.executable() => {
                "j/k scroll · p run · c copy · Esc back"
            }
            (Language::Ru, Some(block)) if block.executable() => {
                "о/л скролл · з запуск · с копировать · Esc назад"
            }
            (Language::En, Some(block)) if !block.animated() => "j/k scroll · c copy · Esc back",
            (Language::Ru, Some(block)) if !block.animated() => {
                "о/л скролл · с копировать · Esc назад"
            }
            (Language::En, _) => "j/k or arrows scroll · Esc back",
            (Language::Ru, _) => "о/л или стрелки — скролл · Esc назад",
        };
        lines.push(Line::from(Span::styled(
            format!(
                "-- READ --  {}/{}  {controls}",
                app.article_cursor() + 1,
                app.article_total_rows().max(1)
            ),
            Style::new().fg(MUTED),
        )));
    } else if let Some(error) = app.articles_error() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            error.to_owned(),
            Style::new().fg(MUTED),
        )));
    } else if app.articles().is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            match app.language() {
                Language::En => "No Markdown files in main/articles yet.",
                Language::Ru => "В main/articles пока нет Markdown-файлов.",
            },
            Style::new().fg(MUTED),
        )));
    } else {
        lines.push(Line::default());
        for (index, article) in app.articles().iter().take(12).enumerate() {
            let selected = index == app.selected_article_index();
            let hovered = app.hovered() == Some(HelpTarget::Article(index));
            let mut spans = vec![
                Span::styled(
                    if selected { "› " } else { "  " },
                    Style::new().fg(INK).bold(),
                ),
                Span::styled(
                    article.title(app.language()).to_owned(),
                    if selected {
                        Style::new().fg(WHITE).bg(CONTROL_ACTIVE).bold()
                    } else if hovered {
                        Style::new().fg(INK).bg(SOFT_GRAY).bold()
                    } else {
                        Style::new().fg(BODY)
                    },
                ),
            ];
            if !article.labels.is_empty() {
                spans.push(Span::raw("  "));
                spans.extend(label_spans(&article.labels));
            }
            lines.push(Line::from(spans));
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            match app.language() {
                Language::En => "j/k select · Enter/o open · e edit · n new · f refresh",
                Language::Ru => "о/л выбор · Enter/щ открыть · у правка · т новая · а обновить",
            },
            Style::new().fg(MUTED),
        )));
    }

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {} ", Tab::Articles.label(app.language()))).fg(INK))
        .padding(if compact {
            Padding::new(1, 1, 1, 0)
        } else {
            Padding::new(2, 2, 1, 1)
        });
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(block)
            .scroll((app.article_scroll(), 0))
            .wrap(Wrap { trim: false }),
        area,
    );
    paint_gradient_border(frame.buffer_mut(), area, INK, MID_GRAY);
}

fn label_spans(labels: &[String]) -> Vec<Span<'static>> {
    labels
        .iter()
        .take(3)
        .flat_map(|label| {
            [
                Span::styled(
                    format!(" {} ", label),
                    Style::new().fg(WHITE).bg(label_color(label)).bold(),
                ),
                Span::raw(" "),
            ]
        })
        .collect()
}

fn skeleton_line(width: u16, row: u8, phase: u8) -> Line<'static> {
    let length = match row {
        0 => width.saturating_mul(4) / 5,
        1 => width.saturating_mul(3) / 5,
        2 => width.saturating_mul(7) / 10,
        3 => width.saturating_mul(1) / 2,
        _ => width.saturating_mul(2) / 3,
    }
    .max(4);
    let spans = (0..length)
        .map(|column| {
            let distance = (u16::from(phase) + column + u16::from(row) * 3) % 24;
            let brightness = if distance < 12 {
                225 + distance as u8
            } else {
                225 + (23 - distance) as u8
            };
            Span::styled(
                " ",
                Style::new().bg(Color::Rgb(brightness, brightness, brightness)),
            )
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

#[cfg(test)]
fn markdown_lines(markdown: &str, images: &[ArticleImage]) -> Vec<Line<'static>> {
    markdown_lines_with_focus(markdown, images, None, 48, 0, None)
}

fn markdown_lines_with_focus(
    markdown: &str,
    images: &[ArticleImage],
    focused_block: Option<usize>,
    block_width: u16,
    animation_phase: u16,
    hovered: Option<HelpTarget>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut code = None::<(usize, String)>;
    let mut block_index = 0_usize;
    let mut front_matter = false;
    for (index, raw) in markdown.lines().enumerate() {
        if raw.trim() == "---" && (index == 0 || front_matter) {
            front_matter = !front_matter;
            continue;
        }
        if front_matter {
            continue;
        }
        if raw.trim_start().starts_with("```") {
            if let Some((index, language)) = code.take() {
                if language != DOT_WELL_LANGUAGE {
                    lines.push(code_block_bottom(block_width, focused_block == Some(index)));
                }
                block_index += 1;
            } else {
                let language = raw
                    .trim()
                    .strip_prefix("```")
                    .unwrap_or_default()
                    .split_whitespace()
                    .next()
                    .filter(|language| !language.is_empty())
                    .unwrap_or("text")
                    .to_lowercase();
                if language == DOT_WELL_LANGUAGE {
                    lines.extend(dot_well_lines(
                        block_width,
                        animation_phase,
                        focused_block == Some(block_index),
                        2,
                        1.0,
                    ));
                } else {
                    let executable = matches!(language.as_str(), "python" | "python3" | "py");
                    lines.push(code_action_line(
                        &language,
                        executable,
                        focused_block == Some(block_index),
                        block_width,
                        block_index,
                        hovered,
                    ));
                }
                code = Some((block_index, language));
            }
            continue;
        }
        if code.is_none()
            && let Some((alt, source)) = markdown_image(raw)
        {
            if let Some(image) = images.iter().find(|image| image.source == source) {
                lines.extend(image_lines(image));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("[image: {alt}]"),
                    Style::new().fg(MUTED).italic(),
                )));
            }
            continue;
        }
        let line = if let Some((_, language)) = &code
            && language == DOT_WELL_LANGUAGE
        {
            continue;
        } else if let Some((index, language)) = &code {
            Line::from(highlight_code_line(
                raw,
                language,
                focused_block == Some(*index),
                block_width,
            ))
        } else if let Some(text) = raw.strip_prefix("### ") {
            Line::from(Span::styled(text.to_owned(), Style::new().fg(INK).bold()))
        } else if let Some(text) = raw.strip_prefix("## ") {
            Line::from(Span::styled(text.to_owned(), Style::new().fg(INK).bold()))
        } else if let Some(text) = raw.strip_prefix("# ") {
            Line::from(Span::styled(
                text.to_owned(),
                Style::new().fg(INK).bold().underlined(),
            ))
        } else if let Some(text) = raw.strip_prefix("- ") {
            Line::from(vec![
                Span::styled("• ", Style::new().fg(INK).bold()),
                Span::styled(text.to_owned(), Style::new().fg(BODY)),
            ])
        } else if let Some(text) = raw.strip_prefix("> ") {
            Line::from(vec![
                Span::styled("│ ", Style::new().fg(MID_GRAY)),
                Span::styled(text.to_owned(), Style::new().fg(GRAPHITE).italic()),
            ])
        } else {
            Line::from(Span::styled(raw.to_owned(), Style::new().fg(BODY)))
        };
        lines.push(line);
    }
    if let Some((index, language)) = code
        && language != DOT_WELL_LANGUAGE
    {
        lines.push(code_block_bottom(block_width, focused_block == Some(index)));
    }
    lines
}

fn dot_well_lines(
    width: u16,
    phase: u16,
    focused: bool,
    well_count: usize,
    well_scale: f64,
) -> Vec<Line<'static>> {
    let background = if focused { CODE_FOCUS } else { PANEL_ALT };
    let border_style = Style::new().fg(MID_GRAY).bg(background);
    let interior_height = usize::from(DOT_WELL_ROWS.saturating_sub(2));
    let interior_width = usize::from(width.saturating_sub(2));
    let mut lines = Vec::with_capacity(usize::from(DOT_WELL_ROWS));

    let label = "╭─ BRAILLE DOT WELL // LIVE ";
    let label = label
        .chars()
        .take(usize::from(width.saturating_sub(1)))
        .collect::<String>();
    let used = label.chars().count().min(usize::from(u16::MAX)) as u16;
    let mut header = vec![Span::styled(label, border_style.bold())];
    if width > used.saturating_add(1) {
        header.push(Span::styled(
            "─".repeat(usize::from(width - used - 1)),
            border_style,
        ));
    }
    if width > 0 {
        header.push(Span::styled("╮", border_style));
    }
    lines.push(Line::from(header).style(Style::new().bg(background)));

    if interior_width > 0 && interior_height > 0 {
        const BRAILLE_COLUMNS: usize = 2;
        const BRAILLE_ROWS: usize = 4;
        const LATTICE_SPACING: usize = 4;

        let sample_width = interior_width.saturating_mul(BRAILLE_COLUMNS);
        let sample_height = interior_height.saturating_mul(BRAILLE_ROWS);
        let mut density = vec![vec![0_u8; sample_width]; sample_height];
        let angle =
            std::f64::consts::TAU * f64::from(phase % DOT_WELL_FRAMES) / f64::from(DOT_WELL_FRAMES);
        let extent = sample_width.min(sample_height).max(1) as f64;
        let middle_x = sample_width.saturating_sub(1) as f64 * 0.5;
        let middle_y = sample_height.saturating_sub(1) as f64 * 0.5;
        let wells = [
            (
                middle_x + extent * (-0.22 + 0.14 * angle.sin() + 0.04 * (angle * 3.0 + 0.7).sin()),
                middle_y
                    + extent
                        * (-0.08 + 0.08 * (angle * 2.0 + 0.2).cos() + 0.035 * (angle * 5.0).sin()),
                0.34 + 0.16 * (0.5 + 0.5 * (angle * 4.0 + 0.4).sin()),
                extent * (0.20 + 0.018 * (angle * 3.0).cos()) * well_scale,
            ),
            (
                middle_x
                    + extent
                        * (0.22 + 0.14 * (angle * 2.0 + 2.1).sin() + 0.04 * (angle * 7.0).cos()),
                middle_y
                    + extent
                        * (0.08
                            + 0.085 * (angle * 3.0 + 1.4).cos()
                            + 0.035 * (angle * 5.0 + 0.8).sin()),
                0.33 + 0.16 * (0.5 + 0.5 * (angle * 3.0 + 2.0).cos()),
                extent * (0.20 + 0.019 * (angle * 4.0 + 1.1).sin()) * well_scale,
            ),
            (
                middle_x
                    + extent
                        * (0.02
                            + 0.12 * (angle * 3.0 + 4.2).sin()
                            + 0.035 * (angle * 8.0 + 0.5).cos()),
                middle_y
                    + extent
                        * (-0.02
                            + 0.11 * (angle * 2.0 + 3.1).cos()
                            + 0.03 * (angle * 7.0 + 1.7).sin()),
                0.36 + 0.17 * (0.5 + 0.5 * (angle * 5.0 + 0.9).sin()),
                extent * (0.22 + 0.02 * (angle * 3.0 + 2.4).cos()) * well_scale,
            ),
        ];

        for source_y in (1..sample_height).step_by(LATTICE_SPACING) {
            let stagger = (source_y / LATTICE_SPACING % 2) * (LATTICE_SPACING / 2);
            for source_x in (stagger..sample_width).step_by(LATTICE_SPACING) {
                let source_x = source_x as f64;
                let source_y = source_y as f64;
                let mut projected_x = source_x;
                let mut projected_y = source_y;
                for (center_x, center_y, compression, sigma) in wells.into_iter().take(well_count) {
                    let dx = source_x - center_x;
                    let dy = source_y - center_y;
                    let radius_squared = dx.mul_add(dx, dy * dy);
                    let gaussian = (-radius_squared / (2.0 * sigma * sigma)).exp();
                    projected_x -= compression * gaussian * dx;
                    projected_y -= compression * gaussian * dy;
                }
                let target_x = (projected_x.round() as isize)
                    .clamp(0, sample_width.saturating_sub(1) as isize)
                    as usize;
                let target_y = (projected_y.round() as isize)
                    .clamp(0, sample_height.saturating_sub(1) as isize)
                    as usize;
                density[target_y][target_x] = density[target_y][target_x].saturating_add(1);
            }
        }

        let collisions = density.clone();
        for (y, row) in collisions.iter().enumerate() {
            for (x, value) in row.iter().copied().enumerate() {
                if value < 2 {
                    continue;
                }
                for (offset_x, offset_y) in [(1_isize, 0_isize), (0, 1), (-1, 0), (0, -1)] {
                    let neighbor_x = x as isize + offset_x;
                    let neighbor_y = y as isize + offset_y;
                    if neighbor_x >= 0
                        && neighbor_y >= 0
                        && let Some(neighbor) = density
                            .get_mut(neighbor_y as usize)
                            .and_then(|row| row.get_mut(neighbor_x as usize))
                    {
                        *neighbor = (*neighbor).max(1);
                    }
                }
            }
        }

        for cell_y in 0..interior_height {
            let mut spans = Vec::with_capacity(interior_width + 2);
            spans.push(Span::styled("│", border_style));
            for cell_x in 0..interior_width {
                let (glyph, dots, weight) = braille_cell(&density, cell_x, cell_y);
                let style = match (dots, weight) {
                    (5.., _) | (_, 2..) => Style::new().fg(INK).bg(background).bold(),
                    (2..=4, _) => Style::new().fg(GRAPHITE).bg(background),
                    _ => Style::new().fg(MID_GRAY).bg(background),
                };
                spans.push(Span::styled(glyph.to_string(), style));
            }
            spans.push(Span::styled("│", border_style));
            lines.push(Line::from(spans).style(Style::new().bg(background)));
        }
    } else {
        for _ in 0..interior_height {
            lines.push(Line::from(Span::styled("│", border_style)));
        }
    }

    lines.push(code_block_bottom(width, focused));
    lines
}

fn braille_cell(density: &[Vec<u8>], cell_x: usize, cell_y: usize) -> (char, u8, u8) {
    const DOTS: [(usize, usize, u32); 8] = [
        (0, 0, 0x01),
        (0, 1, 0x02),
        (0, 2, 0x04),
        (0, 3, 0x40),
        (1, 0, 0x08),
        (1, 1, 0x10),
        (1, 2, 0x20),
        (1, 3, 0x80),
    ];
    let mut bits = 0_u32;
    let mut count = 0_u8;
    let mut weight = 0_u8;
    for (offset_x, offset_y, bit) in DOTS {
        let value = density[cell_y * 4 + offset_y][cell_x * 2 + offset_x];
        if value > 0 {
            bits |= bit;
            count = count.saturating_add(1);
            weight = weight.max(value);
        }
    }
    let glyph = if bits == 0 {
        ' '
    } else {
        char::from_u32(0x2800 + bits).unwrap_or('·')
    };
    (glyph, count, weight)
}

fn code_action_line(
    language: &str,
    executable: bool,
    focused: bool,
    width: u16,
    block_index: usize,
    hovered: Option<HelpTarget>,
) -> Line<'static> {
    let language = language.to_uppercase().chars().take(10).collect::<String>();
    let background = if focused { CODE_FOCUS } else { PANEL_ALT };
    let border = Style::new().fg(MID_GRAY).bg(background);
    let (run_offset, copy_offset) = code_action_offsets(executable, width);
    let first_action = run_offset
        .or(copy_offset)
        .unwrap_or(width.saturating_sub(1));
    let prefix_limit = usize::from(first_action);
    let mut prefix = format!("╭─ {language}");
    prefix = prefix.chars().take(prefix_limit).collect();
    let mut cursor = prefix.chars().count() as u16;
    let mut spans = vec![Span::styled(prefix, border.bold())];
    push_code_header_fill(&mut spans, &mut cursor, first_action, background);
    if run_offset.is_some() {
        let background = if hovered == Some(HelpTarget::CodeRun(block_index)) {
            CODE_KEYWORD
        } else {
            INK
        };
        spans.push(Span::styled(
            " RUN ",
            Style::new().fg(WHITE).bg(background).bold(),
        ));
        cursor = cursor.saturating_add(5);
        let copy_target = copy_offset.unwrap_or(cursor);
        push_code_header_fill(&mut spans, &mut cursor, copy_target, background);
    }
    if copy_offset.is_some() {
        let background = if hovered == Some(HelpTarget::CodeCopy(block_index)) {
            CODE_KEYWORD
        } else {
            GRAPHITE
        };
        spans.push(Span::styled(
            " COPY ",
            Style::new().fg(WHITE).bg(background).bold(),
        ));
        cursor = cursor.saturating_add(6);
    }
    push_code_header_fill(&mut spans, &mut cursor, width.saturating_sub(1), background);
    if width > 0 {
        spans.push(Span::styled("╮", border));
    }
    Line::from(spans).style(Style::new().bg(background))
}

fn push_code_header_fill(
    spans: &mut Vec<Span<'static>>,
    cursor: &mut u16,
    target: u16,
    background: Color,
) {
    if target > *cursor {
        spans.push(Span::styled(
            "─".repeat(usize::from(target - *cursor)),
            Style::new().fg(MID_GRAY).bg(background),
        ));
        *cursor = target;
    }
}

fn code_block_bottom(width: u16, focused: bool) -> Line<'static> {
    let background = if focused { CODE_FOCUS } else { PANEL_ALT };
    let content = match width {
        0 => String::new(),
        1 => "╰".into(),
        _ => format!("╰{}╯", "─".repeat(usize::from(width - 2))),
    };
    Line::from(Span::styled(
        content,
        Style::new().fg(MID_GRAY).bg(background),
    ))
    .style(Style::new().bg(background))
}

fn highlight_code_line(raw: &str, language: &str, focused: bool, width: u16) -> Vec<Span<'static>> {
    let background = if focused { CODE_FOCUS } else { PANEL_ALT };
    let chars = raw.chars().collect::<Vec<_>>();
    let mut spans = vec![Span::styled("│ ", Style::new().fg(MID_GRAY).bg(background))];
    let mut index = 0;
    while index < chars.len() {
        let comment = match language {
            "python" | "python3" | "py" | "bash" | "sh" | "shell" => chars[index] == '#',
            _ => chars[index] == '/' && chars.get(index + 1) == Some(&'/'),
        };
        if comment {
            spans.push(Span::styled(
                chars[index..].iter().collect::<String>(),
                Style::new().fg(CODE_COMMENT).bg(background).italic(),
            ));
            break;
        }
        if matches!(chars[index], '\'' | '"') {
            let quote = chars[index];
            let start = index;
            index += 1;
            while index < chars.len() {
                if chars[index] == quote && chars.get(index.wrapping_sub(1)) != Some(&'\\') {
                    index += 1;
                    break;
                }
                index += 1;
            }
            spans.push(Span::styled(
                chars[start..index].iter().collect::<String>(),
                Style::new().fg(CODE_STRING).bg(background),
            ));
            continue;
        }
        if chars[index].is_alphabetic() || chars[index] == '_' {
            let start = index;
            index += 1;
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let word = chars[start..index].iter().collect::<String>();
            let style = if code_keyword(language, &word) {
                Style::new().fg(CODE_KEYWORD).bg(background).bold()
            } else {
                Style::new().fg(GRAPHITE).bg(background)
            };
            spans.push(Span::styled(word, style));
            continue;
        }
        if chars[index].is_ascii_digit() {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric() || matches!(chars[index], '.' | '_'))
            {
                index += 1;
            }
            spans.push(Span::styled(
                chars[start..index].iter().collect::<String>(),
                Style::new().fg(CODE_NUMBER).bg(background),
            ));
            continue;
        }
        spans.push(Span::styled(
            chars[index].to_string(),
            Style::new().fg(GRAPHITE).bg(background),
        ));
        index += 1;
    }
    let used = 3_u16.saturating_add(chars.len().min(usize::from(u16::MAX)) as u16);
    if width > used {
        spans.push(Span::styled(
            " ".repeat(usize::from(width - used)),
            Style::new().bg(background),
        ));
    }
    if width >= 3 {
        spans.push(Span::styled("│", Style::new().fg(MID_GRAY).bg(background)));
    }
    spans
}

fn code_keyword(language: &str, word: &str) -> bool {
    match language {
        "python" | "python3" | "py" => matches!(
            word,
            "and"
                | "as"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "elif"
                | "else"
                | "except"
                | "False"
                | "finally"
                | "for"
                | "from"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "None"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "True"
                | "try"
                | "while"
                | "with"
                | "yield"
        ),
        "rust" | "rs" => matches!(
            word,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "Self"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
        ),
        "bash" | "sh" | "shell" => matches!(
            word,
            "case"
                | "do"
                | "done"
                | "elif"
                | "else"
                | "esac"
                | "fi"
                | "for"
                | "function"
                | "if"
                | "in"
                | "then"
                | "until"
                | "while"
        ),
        _ => false,
    }
}

fn markdown_image(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    let rest = line.strip_prefix("![")?;
    let (alt, rest) = rest.split_once("](")?;
    let source = rest.strip_suffix(')')?.trim();
    (!source.is_empty()).then_some((alt.trim(), source))
}

fn markdown_image_offsets<'a>(
    markdown: &str,
    images: &'a [ArticleImage],
) -> Vec<(&'a ArticleImage, u16)> {
    let mut offsets = Vec::new();
    let mut row = 0_u16;
    let mut code = None::<bool>;
    let mut front_matter = false;
    for (index, raw) in markdown.lines().enumerate() {
        if raw.trim() == "---" && (index == 0 || front_matter) {
            front_matter = !front_matter;
            continue;
        }
        if front_matter {
            continue;
        }
        if raw.trim_start().starts_with("```") {
            if let Some(animated) = code.take() {
                if !animated {
                    row = row.saturating_add(1);
                }
            } else {
                let animated = raw
                    .trim()
                    .strip_prefix("```")
                    .unwrap_or_default()
                    .split_whitespace()
                    .next()
                    .is_some_and(|language| language.eq_ignore_ascii_case(DOT_WELL_LANGUAGE));
                row = row.saturating_add(if animated { DOT_WELL_ROWS } else { 1 });
                code = Some(animated);
            }
            continue;
        }
        if code.is_none()
            && let Some((_, source)) = markdown_image(raw)
            && let Some(image) = images.iter().find(|image| image.source == source)
        {
            offsets.push((image, row));
            row = row.saturating_add(image.height.div_ceil(2).max(1));
        } else if !code.unwrap_or(false) {
            row = row.saturating_add(1);
        }
    }
    offsets
}

fn image_lines(image: &ArticleImage) -> Vec<Line<'static>> {
    let width = usize::from(image.width);
    let height = usize::from(image.height);
    if width == 0 || height == 0 {
        return vec![Line::from(Span::styled(
            format!("[image: {}]", image.alt),
            Style::new().fg(MUTED).italic(),
        ))];
    }
    if image.pixels.len() != width.saturating_mul(height).saturating_mul(3) {
        return (0..height.div_ceil(2))
            .map(|_| Line::from(" ".repeat(width)))
            .collect();
    }

    let color_at = |x: usize, y: usize| {
        let offset = (y * width + x) * 3;
        Color::Rgb(
            image.pixels[offset],
            image.pixels[offset + 1],
            image.pixels[offset + 2],
        )
    };
    (0..height)
        .step_by(2)
        .map(|y| {
            let spans = (0..width)
                .map(|x| {
                    let top = color_at(x, y);
                    let bottom = if y + 1 < height {
                        color_at(x, y + 1)
                    } else {
                        PANEL
                    };
                    Span::styled("▀", Style::new().fg(top).bg(bottom))
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

fn render_status_panel(frame: &mut Frame<'_>, area: Rect, app: &App) {
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_ALT)), area);

    let inner = area.inner(Margin::new(2, 2));
    paint_horizontal_background(frame.buffer_mut(), inner, PAPER, SOFT_GRAY);

    let (session, owner, visitor, editor) = match app.language() {
        Language::En => ("SESSION", "owner", "visitor", "editor"),
        Language::Ru => ("СЕССИЯ", "владелец", "гость", "редактор"),
    };
    let status = Text::from(vec![
        Line::from(Span::styled(session, Style::new().fg(MUTED).bold())),
        Line::default(),
        metric_line(
            "identity",
            if app.authenticated() { owner } else { visitor },
        ),
        metric_line(
            "presence",
            if app.owner_online() {
                "online"
            } else {
                "offline"
            },
        ),
        metric_line(
            "write",
            if app.authenticated() {
                editor
            } else {
                "locked"
            },
        ),
        Line::default(),
        Line::from(Span::styled("CPU  ▂▃▅▇▆▄", Style::new().fg(INK))),
        Line::from(Span::styled("NET  ▁▂▄▆█▅", Style::new().fg(GRAPHITE))),
    ]);

    frame.render_widget(
        Paragraph::new(status).block(
            Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(" telemetry ").fg(GRAPHITE))
                .padding(Padding::new(1, 1, 1, 1)),
        ),
        area,
    );
    paint_gradient_border(frame.buffer_mut(), area, GRAPHITE, MID_GRAY);
}

fn render_python_output_panel(frame: &mut Frame<'_>, area: Rect, app: &App) {
    frame.render_widget(Clear, area);
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_ALT)), area);

    let content = if app.python_running() {
        Text::from(vec![
            Line::from(Span::styled("PYODIDE", Style::new().fg(MUTED).bold())),
            Line::default(),
            Line::from(Span::styled(
                "Running selected block…",
                Style::new().fg(BODY),
            )),
        ])
    } else {
        Text::from(
            app.python_output()
                .unwrap_or_default()
                .lines()
                .take(64)
                .map(|line| Line::from(Span::styled(line.to_owned(), Style::new().fg(BODY))))
                .collect::<Vec<_>>(),
        )
    };
    frame.render_widget(
        Paragraph::new(content)
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(Line::from(" python output ").fg(GRAPHITE))
                    .padding(Padding::new(1, 1, 1, 1)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
    paint_gradient_border(frame.buffer_mut(), area, GRAPHITE, MID_GRAY);
    if app.python_output().is_some() && area.width >= 5 {
        let close = Rect::new(area.right().saturating_sub(5), area.top(), 3, 1);
        let hovered = app.hovered() == Some(HelpTarget::PythonOutputClose);
        frame.render_widget(
            Paragraph::new(" x ").style(if hovered {
                Style::new().fg(WHITE).bg(CODE_KEYWORD).bold()
            } else {
                Style::new().fg(INK).bg(SOFT_GRAY).bold()
            }),
            close,
        );
    }
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App, compact: bool) {
    paint_horizontal_background(frame.buffer_mut(), area, PAPER, SOFT_GRAY);

    if app.awaiting_site_key() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" g", Style::new().fg(INK).bold()),
                Span::styled("_  press x to open svetsec.ru", Style::new().fg(BODY)),
            ]))
            .alignment(Alignment::Center),
            area,
        );
        return;
    }

    if app.awaiting_article_g() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" g", Style::new().fg(INK).bold()),
                Span::styled("_  press g for document start", Style::new().fg(BODY)),
            ]))
            .alignment(Alignment::Center),
            area,
        );
        return;
    }

    if compact {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(if app.opened_article().is_some() {
                    "READ  •  j/k scroll  •  Esc back"
                } else {
                    "←/→ tabs  •  1–3 jump"
                })
                .centered(),
                Line::from(if app.authenticated() {
                    "r lang  •  e write"
                } else {
                    "r lang  •  a owner"
                })
                .centered(),
            ]),
            area,
        );
        return;
    }

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    let navigation = if app.opened_article().is_some() {
        Line::from(vec![
            Span::styled(" READ ", Style::new().fg(WHITE).bg(CONTROL_ACTIVE).bold()),
            Span::styled(" j/k or ↑/↓ scroll   ", Style::new().fg(BODY)),
            Span::styled(" BACK ", Style::new().fg(WHITE).bg(CONTROL).bold()),
            Span::styled(" Esc ", Style::new().fg(BODY)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" NAV ", Style::new().fg(WHITE).bg(CONTROL_ACTIVE).bold()),
            Span::styled(" ← → / h l   ", Style::new().fg(BODY)),
            Span::styled(" TABS ", Style::new().fg(WHITE).bg(CONTROL).bold()),
            Span::styled(" 1 2 3   ", Style::new().fg(BODY)),
            Span::styled(" OPEN ", Style::new().fg(WHITE).bg(CONTROL).bold()),
            Span::styled(" g x   ", Style::new().fg(BODY)),
            Span::styled(" QUIT ", Style::new().fg(MUTED)),
            Span::styled(" q   ", Style::new().fg(BODY)),
            Span::styled(" LANG ", Style::new().fg(WHITE).bg(CONTROL).bold()),
            Span::styled(" r   ", Style::new().fg(BODY)),
            Span::styled(
                if app.authenticated() {
                    " WRITE "
                } else {
                    " AUTH "
                },
                Style::new().fg(WHITE).bg(CONTROL).bold(),
            ),
            Span::styled(
                if app.authenticated() { " e" } else { " a" },
                Style::new().fg(BODY),
            ),
        ])
    };
    frame.render_widget(
        Paragraph::new(navigation).alignment(Alignment::Center),
        rows[0],
    );

    if let Some(target) = app.hovered() {
        let help = single_line(
            target.text(app.language(), app.owner_online()),
            rows[1].width.min(56) as usize,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(help, Style::new().fg(BODY))))
                .alignment(Alignment::Right),
            rows[1],
        );
    }
}

fn single_line(text: &str, max_chars: usize) -> String {
    let normalized = text.replace(['\r', '\n'], " ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    if max_chars == 0 {
        return String::new();
    }
    normalized
        .chars()
        .take(max_chars.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

fn metric_line(label: &'static str, value: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<8}"), Style::new().fg(MUTED)),
        Span::styled(value, Style::new().fg(BODY)),
    ])
}

fn layout(area: Rect) -> UiLayout {
    let compact = area.width < MOBILE_BREAKPOINT;
    let header_height = if compact { 4 } else { 3 };
    let footer_height = 2;
    let vertical = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Min(5),
        Constraint::Length(footer_height),
    ])
    .split(area);

    let inner = vertical[0].inner(Margin::new(1, 1));
    let (logo, tabs) = if compact {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);
        let tab_columns = Layout::horizontal([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(rows[1]);
        (rows[0], [tab_columns[0], tab_columns[1], tab_columns[2]])
    } else {
        let columns = Layout::horizontal([
            Constraint::Length(15),
            Constraint::Length(11),
            Constraint::Length(12),
            Constraint::Length(11),
            Constraint::Min(0),
        ])
        .split(inner);
        (columns[0], [columns[1], columns[2], columns[3]])
    };

    let status = (!compact && vertical[1].width >= 76).then(|| {
        Layout::horizontal([
            Constraint::Percentage(68),
            Constraint::Length(1),
            Constraint::Percentage(32),
        ])
        .split(vertical[1])[2]
    });

    UiLayout {
        header: vertical[0],
        content: vertical[1],
        footer: vertical[2],
        logo,
        tabs,
        status,
        compact,
    }
}

fn paint_horizontal_background(buffer: &mut Buffer, area: Rect, start: Color, end: Color) {
    if area.is_empty() {
        return;
    }

    let denominator = area.width.saturating_sub(1).max(1);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let color = interpolate(start, end, x - area.left(), denominator);
            buffer[(x, y)].set_bg(color);
        }
    }
}

fn paint_gradient_border(buffer: &mut Buffer, area: Rect, start: Color, end: Color) {
    if area.is_empty() {
        return;
    }

    let denominator = area.width.saturating_sub(1).max(1);
    for x in area.left()..area.right() {
        let color = interpolate(start, end, x - area.left(), denominator);
        buffer[(x, area.top())].set_fg(color);
        if area.height > 1 {
            buffer[(x, area.bottom() - 1)].set_fg(color);
        }
    }

    if area.width > 1 {
        for y in area.top()..area.bottom() {
            buffer[(area.left(), y)].set_fg(start);
            buffer[(area.right() - 1, y)].set_fg(end);
        }
    }
}

fn interpolate(start: Color, end: Color, position: u16, denominator: u16) -> Color {
    let (Color::Rgb(sr, sg, sb), Color::Rgb(er, eg, eb)) = (start, end) else {
        return start;
    };
    let position = u32::from(position);
    let denominator = u32::from(denominator);
    let channel = |from: u8, to: u8| {
        let from = u32::from(from);
        let to = u32::from(to);
        ((from * (denominator - position) + to * position) / denominator) as u8
    };
    Color::Rgb(channel(sr, er), channel(sg, eg), channel(sb, eb))
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use svetsec_core::{App, ArticleContent, ArticleImage, ArticleSummary, Message, Tab};

    use super::{
        CodeBlockAction, article_at, article_back_area, article_cursor_at, article_viewport_rows,
        code_action_areas, code_action_at, help_target_at, label_color, layout,
        markdown_image_offsets, markdown_lines, markdown_lines_with_focus, native_image_placements,
        python_output_area, python_output_close_area, render, resume_link_area, single_line,
        tab_at,
    };

    #[test]
    fn desktop_ui_renders_on_the_shared_test_backend() {
        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| render(frame, &App::default()))
            .expect("UI should render");
    }

    #[test]
    fn compact_ui_renders_at_a_phone_sized_grid() {
        let backend = TestBackend::new(32, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| render(frame, &App::default()))
            .expect("compact UI should render");
    }

    #[test]
    fn desktop_menu_hit_testing_matches_visual_tabs() {
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(tab_at(area, 16, 1), Some(Tab::Main));
        assert_eq!(tab_at(area, 28, 1), Some(Tab::Articles));
        assert_eq!(tab_at(area, 40, 1), Some(Tab::Info));
        assert_eq!(tab_at(area, 2, 1), None);
        assert_eq!(
            help_target_at(area, 2, 1, &App::default()),
            Some(svetsec_core::HelpTarget::Logo)
        );
    }

    #[test]
    fn hovered_tab_gets_a_distinct_button_background() {
        let area = Rect::new(0, 0, 80, 24);
        let articles = layout(area).tabs[1];
        let mut app = App::default();
        let _ = app.update(Message::Hover(Some(svetsec_core::HelpTarget::Tab(
            Tab::Articles,
        ))));
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal.draw(|frame| render(frame, &app)).unwrap();
        assert_eq!(
            terminal.backend().buffer()[(articles.left(), articles.top())].bg,
            super::SOFT_GRAY
        );
    }

    #[test]
    fn compact_menu_uses_a_second_header_row() {
        let area = Rect::new(0, 0, 36, 20);
        let layout = layout(area);
        assert!(layout.compact);
        assert_eq!(tab_at(area, 3, 2), Some(Tab::Main));
        assert_eq!(tab_at(area, 15, 2), Some(Tab::Articles));
        assert_eq!(tab_at(area, 28, 2), Some(Tab::Info));
    }

    #[test]
    fn pending_g_state_can_be_rendered() {
        let mut app = App::default();
        let _ = app.update(Message::BeginSiteShortcut);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| render(frame, &app))
            .expect("pending shortcut UI should render");
    }

    #[test]
    fn hover_hint_is_limited_to_one_line() {
        let hint = single_line("Очень длинная\nподсказка для элемента", 18);
        assert!(!hint.contains('\n'));
        assert_eq!(hint.chars().count(), 18);
        assert!(hint.ends_with('…'));
    }

    #[test]
    fn article_loading_skeleton_renders() {
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.begin_articles_load();
        let _ = app.update(Message::AdvanceSkeleton);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, &app))
            .expect("skeleton should render");
    }

    #[test]
    fn article_rows_are_mouse_targets() {
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.set_articles(vec![ArticleSummary {
            slug: "one".into(),
            title_en: "One".into(),
            title_ru: "Один".into(),
            published: true,
            source_path: Some("articles/one.md".into()),
            edit_url: None,
            labels: Vec::new(),
        }]);
        assert_eq!(article_at(Rect::new(0, 0, 80, 24), 5, 7, &app), Some(0));
    }

    #[test]
    fn back_and_resume_are_visible_mouse_targets_on_phone_layouts() {
        let area = Rect::new(0, 0, 36, 20);
        let mut article_app = App::default();
        let _ = article_app.update(Message::SelectTab(Tab::Articles));
        article_app.set_opened_article(ArticleContent {
            slug: "one".into(),
            title: "One".into(),
            markdown: "Text".into(),
            images: Vec::new(),
            labels: Vec::new(),
        });
        let back = article_back_area(area, &article_app).expect("back target");
        assert_eq!(
            help_target_at(area, back.left(), back.top(), &article_app),
            Some(svetsec_core::HelpTarget::ArticleBack)
        );

        let mut info_app = App::default();
        let _ = info_app.update(Message::SelectTab(Tab::Info));
        let resume = resume_link_area(area, &info_app).expect("resume target");
        assert_eq!(
            help_target_at(area, resume.left(), resume.top(), &info_app),
            Some(svetsec_core::HelpTarget::Resume)
        );
    }

    #[test]
    fn markdown_images_render_as_colored_half_blocks() {
        let lines = markdown_lines(
            "![Earth](assets/earth.png)",
            &[ArticleImage {
                source: "assets/earth.png".into(),
                alt: "Earth".into(),
                width: 1,
                height: 2,
                pixels: vec![1, 2, 3, 4, 5, 6],
            }],
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "▀");
        assert_eq!(
            lines[0].spans[0].style.fg,
            Some(ratatui::style::Color::Rgb(1, 2, 3))
        );
        assert_eq!(
            lines[0].spans[0].style.bg,
            Some(ratatui::style::Color::Rgb(4, 5, 6))
        );
    }

    #[test]
    fn browser_image_placement_tracks_the_markdown_row() {
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.set_opened_article(ArticleContent {
            slug: "earth".into(),
            title: "Earth".into(),
            markdown: "Paragraph\n\n![Earth](assets/earth.png)".into(),
            images: vec![ArticleImage {
                source: "assets/earth.png".into(),
                alt: "Earth".into(),
                width: 32,
                height: 32,
                pixels: vec![0; 32 * 32 * 3],
            }],
            labels: Vec::new(),
        });
        let placements = native_image_placements(Rect::new(0, 0, 100, 30), &app);
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].source, "assets/earth.png");
        assert_eq!((placements[0].width, placements[0].height), (32, 16));
        assert!(placements[0].y > 0);
        let (panel, compact, _, _, _) = super::article_geometry(Rect::new(0, 0, 100, 30));
        let padding = if compact { 1 } else { 2 };
        assert_eq!(placements[0].x, i32::from(panel.left() + 1 + padding));
    }

    #[test]
    fn info_profile_photo_has_a_rounded_native_placement() {
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Info));
        app.set_profile_image(ArticleImage {
            source: "/assets/profile.jpg".into(),
            alt: "Sviatoslav M.".into(),
            width: 18,
            height: 18,
            pixels: Vec::new(),
        });
        let placements = native_image_placements(Rect::new(0, 0, 100, 30), &app);
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].source, "/assets/profile.jpg");
        assert!(placements[0].rounded);
    }

    #[test]
    fn label_colors_are_stable_and_case_insensitive() {
        assert_eq!(label_color("cryptography"), label_color("CRYPTOGRAPHY"));
        assert_eq!(label_color("Криптография"), label_color("КРИПТОГРАФИЯ"));
        assert_eq!(label_color("Unknown Label"), label_color("unknown label"));
        assert_ne!(label_color("unknown-one"), label_color("unknown-two"));
    }

    #[test]
    fn focused_code_block_has_highlighting_and_action_targets() {
        let area = Rect::new(0, 0, 100, 30);
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.set_opened_article(ArticleContent {
            slug: "code".into(),
            title: "Code".into(),
            markdown: concat!(
                "```rust\nlet value = 1;\n```\n",
                "```python\nfor value in range(3): # comment\n```"
            )
            .into(),
            images: Vec::new(),
            labels: Vec::new(),
        });
        app.set_article_viewport_rows(article_viewport_rows(area));
        let python = app.article_code_block(1).unwrap();
        let _ = app.update(Message::SelectArticleCursor(python.start_row));

        let actions = code_action_areas(area, &app);
        assert_eq!(actions.len(), 3);
        let (run, run_area) = actions
            .iter()
            .find(|(action, _)| matches!(action, CodeBlockAction::Run { block: 1, .. }))
            .copied()
            .unwrap();
        let (_, _, content_left, _, _) = super::article_geometry(area);
        let block_width = super::article_content_width(area).saturating_sub(2);
        let (run_offset, _) = super::code_action_offsets(true, block_width);
        assert_eq!(run_area.left(), content_left + run_offset.unwrap());
        assert_eq!(
            code_action_at(area, run_area.left(), run_area.top(), &app),
            Some(run)
        );
        assert_eq!(
            super::article_position_at(area, content_left, run_area.top(), &app),
            Some((python.start_row, 0))
        );
        assert_eq!(
            article_cursor_at(area, run_area.left(), run_area.top(), &app),
            Some(python.start_row)
        );

        let lines = markdown_lines_with_focus(
            "```python\nfor value in range(3): # comment\n```",
            &[],
            Some(0),
            48,
            0,
            None,
        );
        assert_eq!(lines.len(), 3);
        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered[0].starts_with("╭─ PYTHON"));
        assert!(rendered[0].contains(" RUN "));
        assert!(rendered[0].contains(" COPY "));
        assert!(rendered[1].starts_with("│ "));
        assert!(rendered[1].ends_with('│'));
        assert!(rendered[2].starts_with('╰'));
        assert!(rendered[2].ends_with('╯'));
        assert!(rendered.iter().all(|line| line.chars().count() == 48));
        let keyword = lines[1]
            .spans
            .iter()
            .find(|span| span.content == "for")
            .unwrap();
        let number = lines[1]
            .spans
            .iter()
            .find(|span| span.content == "3")
            .unwrap();
        let comment = lines[1]
            .spans
            .iter()
            .find(|span| span.content == "# comment")
            .unwrap();
        assert_eq!(keyword.style.fg, Some(super::CODE_KEYWORD));
        assert_eq!(number.style.fg, Some(super::CODE_NUMBER));
        assert_eq!(comment.style.fg, Some(super::CODE_COMMENT));
        assert!(
            lines[1]
                .spans
                .iter()
                .all(|span| span.style.bg == Some(super::CODE_FOCUS))
        );
    }

    #[test]
    fn native_image_offsets_include_code_action_headers() {
        let images = [ArticleImage {
            source: "image.png".into(),
            alt: "Image".into(),
            width: 4,
            height: 4,
            pixels: vec![0; 4 * 4 * 3],
        }];
        let offsets =
            markdown_image_offsets("```python\nprint(42)\n```\n![Image](image.png)", &images);
        assert_eq!(offsets.len(), 1);
        assert_eq!(offsets[0].1, 3);
    }

    #[test]
    fn dot_well_is_a_fixed_height_animation_without_code_actions() {
        let first = markdown_lines_with_focus("```dot-well\n```", &[], Some(0), 48, 0, None);
        let next = markdown_lines_with_focus("```dot-well\n```", &[], Some(0), 48, 1, None);
        let later = markdown_lines_with_focus("```dot-well\n```", &[], Some(0), 48, 3, None);
        assert_eq!(first.len(), usize::from(super::DOT_WELL_ROWS));
        assert_eq!(later.len(), first.len());
        assert!(first.iter().all(|line| {
            line.spans
                .iter()
                .map(|span| span.content.chars().count())
                .sum::<usize>()
                == 48
        }));
        assert!(first.iter().flat_map(|line| &line.spans).all(|span| {
            span.style.fg != Some(super::CODE_KEYWORD) && !span.content.contains(['●', '○'])
        }));
        let text = |lines: &[ratatui::text::Line<'static>]| {
            lines
                .iter()
                .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
                .collect::<String>()
        };
        let first = text(&first);
        let next = text(&next);
        let later = text(&later);
        assert!(first.contains("BRAILLE DOT WELL"));
        assert!(
            first
                .chars()
                .filter(|character| ('\u{2801}'..='\u{28ff}').contains(character))
                .count()
                > 100
        );
        let adjacent_changes = first
            .chars()
            .zip(next.chars())
            .filter(|(left, right)| left != right)
            .count();
        assert!((1..80).contains(&adjacent_changes));
        assert_ne!(first, later);
    }

    #[test]
    fn article_loading_uses_three_larger_wells() {
        let three = super::dot_well_lines(48, 0, false, 3, 1.35);
        let embedded = super::dot_well_lines(48, 0, false, 2, 1.0);
        assert_eq!(three.len(), usize::from(super::DOT_WELL_ROWS));
        assert_ne!(three, embedded);
    }

    #[test]
    fn finished_python_output_replaces_telemetry_and_has_a_close_target() {
        let area = Rect::new(0, 0, 100, 30);
        let mut app = App::default();
        let _ = app.update(Message::SelectTab(Tab::Articles));
        app.set_opened_article(ArticleContent {
            slug: "python".into(),
            title: "Python".into(),
            markdown: "```python\nprint(42)\n```".into(),
            images: Vec::new(),
            labels: Vec::new(),
        });
        app.finish_python_run("42");

        let output = python_output_area(area, &app).expect("output panel");
        let content = layout(area).content;
        assert!(output.left() > content.left() + content.width / 2);
        let close = python_output_close_area(area, &app).expect("close button");
        assert_eq!(
            help_target_at(area, close.left(), close.top(), &app),
            Some(svetsec_core::HelpTarget::PythonOutputClose)
        );
    }

    #[test]
    fn native_image_offsets_treat_dot_well_as_one_fixed_height_panel() {
        let images = [ArticleImage {
            source: "image.png".into(),
            alt: "Image".into(),
            width: 4,
            height: 4,
            pixels: vec![0; 4 * 4 * 3],
        }];
        let offsets = markdown_image_offsets("```dot-well\n```\n![Image](image.png)", &images);
        assert_eq!(offsets[0].1, super::DOT_WELL_ROWS);
    }
}
