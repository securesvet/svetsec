use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap},
};
use svetsec_core::{App, ArticleImage, HelpTarget, Language, Tab};

const WHITE: Color = Color::Rgb(255, 255, 255);
const PAPER: Color = Color::Rgb(250, 250, 248);
const SOFT_GRAY: Color = Color::Rgb(232, 233, 231);
const PANEL: Color = Color::Rgb(255, 255, 255);
const PANEL_ALT: Color = Color::Rgb(245, 246, 244);
const INK: Color = Color::Rgb(24, 26, 28);
const GRAPHITE: Color = Color::Rgb(70, 74, 78);
const MID_GRAY: Color = Color::Rgb(148, 152, 154);
const BODY: Color = Color::Rgb(54, 58, 61);
const MUTED: Color = Color::Rgb(105, 109, 112);
const ONLINE_GREEN: Color = Color::Rgb(26, 166, 90);
const MOBILE_BREAKPOINT: u16 = 56;

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
}

#[must_use]
pub fn tab_at(area: Rect, column: u16, row: u16) -> Option<Tab> {
    layout(area)
        .tabs
        .into_iter()
        .zip(Tab::ALL)
        .find_map(|(area, tab)| area.contains((column, row).into()).then_some(tab))
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
pub fn help_target_at(area: Rect, column: u16, row: u16) -> Option<HelpTarget> {
    let layout = layout(area);
    let position = (column, row).into();
    if layout.logo.contains(position) {
        return Some(HelpTarget::Logo);
    }
    if let Some((_, tab)) = layout
        .tabs
        .into_iter()
        .zip(Tab::ALL)
        .find(|(area, _)| area.contains(position))
    {
        return Some(HelpTarget::Tab(tab));
    }
    if layout.status.is_some_and(|area| area.contains(position)) {
        return Some(HelpTarget::Status);
    }
    (layout.content.contains(position)).then_some(HelpTarget::Articles)
}

#[must_use]
pub fn article_image_placements<'a>(area: Rect, app: &'a App) -> Vec<ArticleImagePlacement<'a>> {
    if app.selected() != Tab::Articles || app.language_notice() {
        return Vec::new();
    }
    let Some(article) = app.opened_article() else {
        return Vec::new();
    };
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
    let compact = area.width < MOBILE_BREAKPOINT;
    let horizontal_padding = if compact { 1 } else { 2 };
    let bottom_padding = if compact { 0 } else { 1 };
    let content_left = i32::from(primary.left()) + 1 + horizontal_padding;
    let content_right = i32::from(primary.right()) - 1 - horizontal_padding;
    let content_top = i32::from(primary.top()) + 2;
    let content_bottom = i32::from(primary.bottom()) - 1 - bottom_padding;
    let markdown_top = content_top + 4 - i32::from(app.article_scroll());

    markdown_image_offsets(&article.markdown, &article.images)
        .into_iter()
        .filter_map(|(image, offset)| {
            let x = content_left;
            let y = markdown_top + i32::from(offset);
            let width = image.width;
            let height = image.height.div_ceil(2);
            let right = x + i32::from(width);
            let bottom = y + i32::from(height);
            let clip_top = (content_top - y).max(0).min(i32::from(height)) as u16;
            let clip_right = (right - content_right).max(0).min(i32::from(width)) as u16;
            let clip_bottom = (bottom - content_bottom).max(0).min(i32::from(height)) as u16;
            (clip_top + clip_bottom < height && clip_right < width).then_some(
                ArticleImagePlacement {
                    source: &image.source,
                    alt: &image.alt,
                    x,
                    y,
                    width,
                    height,
                    clip_top,
                    clip_right,
                    clip_bottom,
                },
            )
        })
        .collect()
}

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(Block::new().style(Style::new().bg(WHITE)), area);

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

        if tab == app.selected() {
            paint_horizontal_background(frame.buffer_mut(), tab_area, INK, GRAPHITE);
        }

        frame.render_widget(
            Paragraph::new(tab.label(app.language()))
                .alignment(Alignment::Center)
                .style(if tab == app.selected() {
                    selected_style
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
    if area.width >= 76 {
        let columns = Layout::horizontal([
            Constraint::Percentage(68),
            Constraint::Length(1),
            Constraint::Percentage(32),
        ])
        .split(area);
        render_primary_panel(frame, columns[0], app, false);
        render_status_panel(frame, columns[2], app);
    } else {
        render_primary_panel(frame, area, app, true);
    }
}

fn render_primary_panel(frame: &mut Frame<'_>, area: Rect, app: &App, compact: bool) {
    if app.selected() == Tab::Articles {
        render_articles_panel(frame, area, app, compact);
        return;
    }
    frame.render_widget(Block::new().style(Style::new().bg(PANEL)), area);

    let content = vec![
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
        Line::from(Span::styled(
            app.selected().description(app.language()),
            Style::new().fg(BODY),
        )),
        Line::default(),
        Line::from(vec![
            Span::styled("SIGNAL  ", Style::new().fg(MUTED)),
            Span::styled("▁▂▃▅▇█▇▆▄▃▅▆", Style::new().fg(GRAPHITE)),
        ]),
    ];

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
    let mut lines = vec![Line::from(vec![
        Span::styled("● SYNC", Style::new().fg(INK).bold()),
        Span::styled("  //  GITHUB main/articles", Style::new().fg(MUTED)),
    ])];

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
        for row in 0..5 {
            lines.push(skeleton_line(
                area.width.saturating_sub(if compact { 6 } else { 10 }),
                row,
                app.skeleton_phase(),
            ));
        }
    } else if let Some(article) = app.opened_article() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            article.title.clone(),
            Style::new().fg(INK).bold(),
        )));
        lines.push(Line::default());
        lines.extend(markdown_lines(&article.markdown, &article.images));
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            match app.language() {
                Language::En => "↑/↓ or j/k scroll · Esc back",
                Language::Ru => "↑/↓ или о/л прокрутка · Esc назад",
            },
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
            lines.push(Line::from(vec![
                Span::styled(
                    if selected { "› " } else { "  " },
                    Style::new().fg(INK).bold(),
                ),
                Span::styled(
                    article.title(app.language()).to_owned(),
                    if selected {
                        Style::new().fg(WHITE).bg(INK).bold()
                    } else {
                        Style::new().fg(BODY)
                    },
                ),
            ]));
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

fn markdown_lines(markdown: &str, images: &[ArticleImage]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut code = false;
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
            code = !code;
            continue;
        }
        if !code && let Some((alt, source)) = markdown_image(raw) {
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
        let line = if code {
            Line::from(Span::styled(
                format!("  {raw}"),
                Style::new().fg(GRAPHITE).bg(PANEL_ALT),
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
    lines
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
    let mut code = false;
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
            code = !code;
            continue;
        }
        if !code
            && let Some((_, source)) = markdown_image(raw)
            && let Some(image) = images.iter().find(|image| image.source == source)
        {
            offsets.push((image, row));
            row = row.saturating_add(image.height.div_ceil(2).max(1));
        } else {
            row = row.saturating_add(1);
        }
    }
    offsets
}

fn image_lines(image: &ArticleImage) -> Vec<Line<'static>> {
    let width = usize::from(image.width);
    let height = usize::from(image.height);
    if width == 0
        || height == 0
        || image.pixels.len() != width.saturating_mul(height).saturating_mul(3)
    {
        return vec![Line::from(Span::styled(
            format!("[image: {}]", image.alt),
            Style::new().fg(MUTED).italic(),
        ))];
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

    if compact {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("←/→ tabs  •  1–3 jump").centered(),
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
    let navigation = Line::from(vec![
        Span::styled(" NAV ", Style::new().fg(WHITE).bg(INK).bold()),
        Span::styled(" ← → / h l   ", Style::new().fg(BODY)),
        Span::styled(" TABS ", Style::new().fg(WHITE).bg(GRAPHITE).bold()),
        Span::styled(" 1 2 3   ", Style::new().fg(BODY)),
        Span::styled(" OPEN ", Style::new().fg(WHITE).bg(MID_GRAY).bold()),
        Span::styled(" g x   ", Style::new().fg(BODY)),
        Span::styled(" QUIT ", Style::new().fg(MUTED)),
        Span::styled(" q   ", Style::new().fg(BODY)),
        Span::styled(" LANG ", Style::new().fg(WHITE).bg(GRAPHITE).bold()),
        Span::styled(" r   ", Style::new().fg(BODY)),
        Span::styled(
            if app.authenticated() {
                " WRITE "
            } else {
                " AUTH "
            },
            Style::new().fg(WHITE).bg(MID_GRAY).bold(),
        ),
        Span::styled(
            if app.authenticated() { " e" } else { " a" },
            Style::new().fg(BODY),
        ),
    ]);
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
        article_at, article_image_placements, help_target_at, layout, markdown_lines, render,
        single_line, tab_at,
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
            help_target_at(area, 2, 1),
            Some(svetsec_core::HelpTarget::Logo)
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
        }]);
        assert_eq!(article_at(Rect::new(0, 0, 80, 24), 5, 7, &app), Some(0));
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
        });
        let placements = article_image_placements(Rect::new(0, 0, 100, 30), &app);
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].source, "assets/earth.png");
        assert_eq!((placements[0].width, placements[0].height), (32, 16));
        assert!(placements[0].y > 0);
    }
}
