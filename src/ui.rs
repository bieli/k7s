use crate::app::{
    App, APP_HEADER_TITLE, APP_HEADER_TITLE_K8S_VER, APP_HEADER_TITLE_LEFT, APP_HEADER_TITLE_RIGHT,
};
use crate::app::{DetailModal, PANE_CONFIGS};
use crate::resources::ResourceRow;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState, Wrap,
    },
    Frame,
};

fn ui_header(f: &mut Frame, area: Rect, app: &App) {
    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled(
            APP_HEADER_TITLE_LEFT,
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        ),
        Span::styled(
            format!("{}", env!("CARGO_PKG_NAME")),
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::White),
        ),
        Span::styled(
            format!(" {} ", APP_HEADER_TITLE),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(
            format!("{}{}", APP_HEADER_TITLE_K8S_VER, app.server_version),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            APP_HEADER_TITLE_RIGHT,
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        ),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(paragraph, area);
}

fn ui_namespaces(f: &mut Frame, area: Rect, app: &App) {
    let spans: Vec<Span> = app
        .namespaces
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let style = if i == app.selected_ns_index {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            Span::styled(format!("[{}] {}  ", i, n), style)
        })
        .collect();
    f.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .title(" Namespaces (select by keypress: 0 - 9) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        ),
        area,
    );
}

pub fn ui_create(f: &mut Frame, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(f.size());

    ui_header(f, root[0], app);
    ui_namespaces(f, root[1], app);

    let areas: Vec<Rect> = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(root[2])
        .iter()
        .flat_map(|row| {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(*row)
                .to_vec()
        })
        .collect();

    for (cfg, area) in PANE_CONFIGS.iter().zip(areas.iter()) {
        let items = app.rows.get(&cfg.pane).map(Vec::as_slice).unwrap_or(&[]);
        let state = app.states.get_mut(&cfg.pane).unwrap();
        let active = app.active_pane == cfg.pane;
        ui_render_table(
            f,
            *area,
            state,
            active,
            cfg.title,
            cfg.headers,
            items,
            cfg.constraints,
        );
    }

    if let Some(detail) = &mut app.detail {
        ui_render_detail(f, detail);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}

fn ui_render_detail(f: &mut Frame, detail: &mut DetailModal) {
    let area = centered_rect(80, 80, f.size());
    let inner_h = area.height.saturating_sub(2) as usize;

    detail.visible_height = inner_h;

    let max_scroll = detail.lines.len().saturating_sub(inner_h);
    if detail.scroll > max_scroll {
        detail.scroll = max_scroll;
    }

    let visible: Vec<Line> = detail
        .lines
        .iter()
        .skip(detail.scroll)
        .take(inner_h)
        .map(|l| {
            if l.contains("━━━") {
                Line::from(Span::styled(
                    l.clone(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
            } else if l.trim_start().starts_with("kubectl") {
                Line::from(Span::styled(l.clone(), Style::default().fg(Color::Yellow)))
            } else if l.trim_start().starts_with("Esc") {
                Line::from(Span::styled(
                    l.clone(),
                    Style::default().fg(Color::DarkGray),
                ))
            } else if l.trim_start().starts_with("Container:") {
                Line::from(Span::styled(
                    l.clone(),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                let trimmed = l.trim_start();
                let indent = l.len() - trimmed.len();
                if let Some(pos) = trimmed.find("  ") {
                    let label = &trimmed[..pos];
                    let value = trimmed[pos..].trim_start();
                    Line::from(vec![
                        Span::raw(" ".repeat(indent)),
                        Span::styled(format!("{:<28}", label), Style::default().fg(Color::Blue)),
                        Span::raw("  "),
                        Span::styled(value.to_string(), Style::default().fg(Color::White)),
                    ])
                } else {
                    Line::from(Span::styled(l.clone(), Style::default().fg(Color::Gray)))
                }
            }
        })
        .collect();

    let scroll_hint = if detail.lines.len() > inner_h {
        format!(" [{}/{}] ", detail.scroll + 1, max_scroll + 1)
    } else {
        String::new()
    };

    let mut scrollbar_state = ScrollbarState::new(detail.lines.len()).position(detail.scroll);

    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(visible)
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("{}{}", detail.title, scroll_hint),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓")),
        area,
        &mut scrollbar_state,
    );
}

fn ui_render_table(
    f: &mut Frame,
    area: Rect,
    state: &mut TableState,
    is_active: bool,
    title: &str,
    headers: &[&str],
    items: &[ResourceRow],
    constraints: &[Constraint],
) {
    let border_color = if is_active {
        Color::Green
    } else {
        Color::White
    };
    let rows = items.iter().map(|item| {
        let cells = std::iter::once(Cell::from(item.name.clone()))
            .chain(item.data.iter().map(|d| Cell::from(d.clone())));
        Row::new(cells)
    });
    let table = Table::new(rows, constraints)
        .header(
            Row::new(headers.iter().map(|h| Cell::from(*h))).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(
            Block::default()
                .title(format!(" {} ", title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol(">> ");
    f.render_stateful_widget(table, area, state);
}
