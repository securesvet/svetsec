use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap},
};
use svetsec_core::{App, Tab};

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
const MOBILE_BREAKPOINT: u16 = 56;

struct UiLayout {
    header: Rect,
    content: Rect,
    footer: Rect,
    logo: Rect,
    tabs: [Rect; 3],
    compact: bool,
}

#[must_use]
pub fn tab_at(area: Rect, column: u16, row: u16) -> Option<Tab> {
    layout(area)
        .tabs
        .into_iter()
        .zip(Tab::ALL)
        .find_map(|(area, tab)| area.contains((column, row).into()).then_some(tab))
}

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(Block::new().style(Style::new().bg(WHITE)), area);

    let layout = layout(area);
    render_header(frame, &layout, app.selected());
    render_content(frame, layout.content, app.selected(), layout.compact);
    render_footer(frame, layout.footer, app, layout.compact);
}

fn render_header(frame: &mut Frame<'_>, layout: &UiLayout, selected: Tab) {
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
            Span::styled("● ", Style::new().fg(INK)),
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

        if tab == selected {
            paint_horizontal_background(frame.buffer_mut(), tab_area, INK, GRAPHITE);
        }

        frame.render_widget(
            Paragraph::new(tab.label())
                .alignment(Alignment::Center)
                .style(if tab == selected {
                    selected_style
                } else {
                    idle_style
                }),
            tab_area,
        );
    }
}

fn render_content(frame: &mut Frame<'_>, area: Rect, selected: Tab, compact: bool) {
    if !compact && area.width >= 76 {
        let columns = Layout::horizontal([
            Constraint::Percentage(68),
            Constraint::Length(1),
            Constraint::Percentage(32),
        ])
        .split(area);
        render_primary_panel(frame, columns[0], selected, false);
        render_status_panel(frame, columns[2]);
    } else {
        render_primary_panel(frame, area, selected, compact);
    }
}

fn render_primary_panel(frame: &mut Frame<'_>, area: Rect, selected: Tab, compact: bool) {
    frame.render_widget(Block::new().style(Style::new().bg(PANEL)), area);

    let content = Text::from(vec![
        Line::from(vec![
            Span::styled("● ONLINE", Style::new().fg(INK).bold()),
            Span::styled("  //  RUST + WASM", Style::new().fg(MUTED)),
        ]),
        Line::default(),
        Line::from(Span::styled(
            selected.title(),
            Style::new().fg(INK).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(selected.description(), Style::new().fg(BODY))),
        Line::default(),
        Line::from(vec![
            Span::styled("SIGNAL  ", Style::new().fg(MUTED)),
            Span::styled("▁▂▃▅▇█▇▆▄▃▅▆", Style::new().fg(GRAPHITE)),
        ]),
    ]);

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {} ", selected.label())).fg(INK))
        .padding(if compact {
            Padding::new(1, 1, 1, 0)
        } else {
            Padding::new(2, 2, 1, 1)
        });

    frame.render_widget(
        Paragraph::new(content)
            .block(block)
            .wrap(Wrap { trim: true }),
        area,
    );
    paint_gradient_border(frame.buffer_mut(), area, INK, MID_GRAY);
}

fn render_status_panel(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_ALT)), area);

    let inner = area.inner(Margin::new(2, 2));
    paint_horizontal_background(frame.buffer_mut(), inner, PAPER, SOFT_GRAY);

    let status = Text::from(vec![
        Line::from(Span::styled("SESSION", Style::new().fg(MUTED).bold())),
        Line::default(),
        metric_line("runtime", "native / wasm"),
        metric_line("render", "true color"),
        metric_line("theme", "paper / graphite"),
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

    let text = if app.awaiting_site_key() {
        Text::from(Line::from(vec![
            Span::styled(" g", Style::new().fg(INK).bold()),
            Span::styled("_  press x to open svetsec.ru", Style::new().fg(BODY)),
        ]))
    } else if compact {
        Text::from(vec![
            Line::from("←/→ tabs  •  1–3 jump").centered(),
            Line::from("gx site  •  q quit").centered(),
        ])
    } else {
        Text::from(Line::from(vec![
            Span::styled(" NAV ", Style::new().fg(WHITE).bg(INK).bold()),
            Span::styled(" ← → / h l   ", Style::new().fg(BODY)),
            Span::styled(" TABS ", Style::new().fg(WHITE).bg(GRAPHITE).bold()),
            Span::styled(" 1 2 3   ", Style::new().fg(BODY)),
            Span::styled(" OPEN ", Style::new().fg(WHITE).bg(MID_GRAY).bold()),
            Span::styled(" g x   ", Style::new().fg(BODY)),
            Span::styled(" QUIT ", Style::new().fg(MUTED)),
            Span::styled(" q", Style::new().fg(BODY)),
        ]))
    };

    frame.render_widget(Paragraph::new(text).alignment(Alignment::Center), area);
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
    let footer_height = if compact { 2 } else { 1 };
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
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Min(0),
        ])
        .split(inner);
        (columns[0], [columns[1], columns[2], columns[3]])
    };

    UiLayout {
        header: vertical[0],
        content: vertical[1],
        footer: vertical[2],
        logo,
        tabs,
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
    use svetsec_core::{App, Message, Tab};

    use super::{layout, render, tab_at};

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
        assert_eq!(tab_at(area, 25, 1), Some(Tab::Articles));
        assert_eq!(tab_at(area, 37, 1), Some(Tab::Info));
        assert_eq!(tab_at(area, 2, 1), None);
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
}
