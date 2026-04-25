use crate::app::PaneConfig;
use crate::app::{
    App, DetailModal, APP_HEADER_TITLE, APP_HEADER_TITLE_K8S_VER, APP_HEADER_TITLE_LEFT,
    APP_HEADER_TITLE_RIGHT, PANE_CONFIGS,
};
use crate::resources::ResourceRow;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState, Wrap,
    },
    Frame,
};

pub struct UiCtx<'a, 'b> {
    pub frame: &'a mut Frame<'b>,
    pub app: &'a mut App,
}

struct Styles;

impl Styles {
    fn header_left() -> Style {
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Cyan)
    }

    fn header_title() -> Style {
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::White)
    }

    fn header_meta() -> Style {
        Style::default().fg(Color::Yellow)
    }

    fn header_dim() -> Style {
        Style::default().fg(Color::DarkGray)
    }
}

pub fn ui_create(f: &mut Frame, app: &mut App) {
    let mut ctx = UiCtx { frame: f, app };

    let root = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .split(ctx.frame.size());

    render_header(&mut ctx, root[0]);
    render_namespaces(&mut ctx, root[1]);
    render_panes(&mut ctx, root[2]);

    if let Some(detail) = &mut ctx.app.detail {
        render_detail(ctx.frame, detail);
    }
}

fn render_header(ctx: &mut UiCtx, area: Rect) {
    let app = &mut ctx.app;

    let line = Line::from(vec![
        Span::styled(APP_HEADER_TITLE_LEFT, Styles::header_left()),
        Span::styled(env!("CARGO_PKG_NAME"), Styles::header_title()),
        Span::styled(format!(" {} ", APP_HEADER_TITLE), Styles::header_meta()),
        Span::styled(
            format!("{}{}", APP_HEADER_TITLE_K8S_VER, app.server_version),
            Styles::header_dim(),
        ),
        Span::styled(APP_HEADER_TITLE_RIGHT, Styles::header_left()),
    ]);

    ctx.frame
        .render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}

fn render_namespaces(ctx: &mut UiCtx, area: Rect) {
    let app = &mut ctx.app;

    let spans = app
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
        .collect::<Vec<_>>();

    ctx.frame.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .title(" Namespaces (0-9) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        ),
        area,
    );
}

fn render_panes(ctx: &mut UiCtx, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ])
    .split(area);

    let grid: Vec<Rect> = rows
        .iter()
        .flat_map(|row| {
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(*row)
                .to_vec()
        })
        .collect();

    for (cfg, area) in PANE_CONFIGS.iter().zip(grid.iter()) {
        render_table_pane(ctx, cfg, *area);
    }
}

struct TableRender<'a> {
    area: Rect,
    state: &'a mut TableState,
    active: bool,
    title: &'a str,
    headers: &'a [&'a str],
    items: &'a [ResourceRow],
    constraints: &'a [Constraint],
}

fn render_table_pane(ctx: &mut UiCtx, cfg: &PaneConfig, area: Rect) {
    let app = &mut ctx.app;

    let items = app.rows.get(&cfg.pane).map(Vec::as_slice).unwrap_or(&[]);
    let state = app.states.get_mut(&cfg.pane).unwrap();
    let active = app.active_pane == cfg.pane;

    render_table(
        ctx.frame,
        TableRender {
            area,
            state,
            active,
            title: cfg.title,
            headers: cfg.headers,
            items,
            constraints: cfg.constraints,
        },
    );
}

fn render_table(f: &mut Frame, cfg: TableRender) {
    let border = if cfg.active {
        Color::Green
    } else {
        Color::White
    };

    let rows = cfg.items.iter().map(|item| {
        Row::new(
            std::iter::once(Cell::from(item.name.clone()))
                .chain(item.data.iter().map(|d| Cell::from(d.clone()))),
        )
    });

    let table = Table::new(rows, cfg.constraints)
        .header(
            Row::new(cfg.headers.iter().map(|h| Cell::from(*h))).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(
            Block::default()
                .title(format!(" {} ", cfg.title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol(">> ");

    f.render_stateful_widget(table, cfg.area, cfg.state);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vert = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vert[1])[1]
}

fn render_detail(f: &mut Frame, detail: &mut DetailModal) {
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
        .map(|l| Line::from(Span::raw(l.clone())))
        .collect();

    let mut scrollbar_state = ScrollbarState::new(detail.lines.len()).position(detail.scroll);

    f.render_widget(Clear, area);

    f.render_widget(
        Paragraph::new(visible)
            .block(
                Block::default()
                    .title(detail.title.as_str())
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );

    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        area,
        &mut scrollbar_state,
    );
}
