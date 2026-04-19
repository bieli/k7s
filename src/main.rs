use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet};
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{Namespace, Pod, Service};
use kube::Client;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState, Wrap,
    },
    Frame, Terminal,
};
use std::{collections::BTreeMap, io, time::Duration, time::Instant};

mod resources;
use crate::resources::{fetch_cluster_resources, fetch_resources, ResourceRow};

mod describe_resources;
use crate::describe_resources::{
    describe_daemonset, describe_deployment, describe_job, describe_pod, describe_replicaset,
    describe_service,
};

const APP_HEADER_TITLE: &str = concat!(
    "Kubernetes Resources Viewer by @bieli v",
    env!("CARGO_PKG_VERSION")
);
const APP_HEADER_TITLE_LEFT: &str = "--- [ ";
const APP_HEADER_TITLE_RIGHT: &str = " ] ---";
const APP_HEADER_TITLE_K8S_VER: &str = "| K8s API: v";
const TICKS_DELAY: u32 = 1000;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Pane {
    Pods,
    Services,
    Deployments,
    ReplicaSets,
    DaemonSets,
    Jobs,
}

impl Pane {
    fn all() -> &'static [Pane] {
        &[
            Pane::Pods,
            Pane::Services,
            Pane::Deployments,
            Pane::ReplicaSets,
            Pane::DaemonSets,
            Pane::Jobs,
        ]
    }
}

struct PaneConfig {
    pane: Pane,
    title: &'static str,
    headers: &'static [&'static str],
    constraints: &'static [Constraint],
}

const PANE_CONFIGS: &[PaneConfig] = &[
    PaneConfig {
        pane: Pane::Pods,
        title: "Pods",
        headers: &["NAME", "READY", "STATUS", "RESTARTS", "AGE"],
        constraints: &[
            Constraint::Percentage(35),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    },
    PaneConfig {
        pane: Pane::Services,
        title: "Services",
        headers: &[
            "NAME",
            "TYPE",
            "CLUSTER-IP",
            "EXTERNAL-IP",
            "PORT(S)",
            "AGE",
        ],
        constraints: &[
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(25),
            Constraint::Percentage(10),
        ],
    },
    PaneConfig {
        pane: Pane::Deployments,
        title: "Deployments",
        headers: &["NAME", "READY", "UP-TO-DATE", "AVAILABLE", "AGE"],
        constraints: &[
            Constraint::Percentage(35),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    },
    PaneConfig {
        pane: Pane::ReplicaSets,
        title: "ReplicaSets",
        headers: &["NAME", "DESIRED", "CURRENT", "READY", "AGE"],
        constraints: &[
            Constraint::Percentage(35),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    },
    PaneConfig {
        pane: Pane::DaemonSets,
        title: "DaemonSets",
        headers: &["NAME", "DESIRED", "CURRENT", "READY", "AGE"],
        constraints: &[
            Constraint::Percentage(35),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    },
    PaneConfig {
        pane: Pane::Jobs,
        title: "Jobs",
        headers: &["NAME", "COMPLETIONS", "ACTIVE", "FAILED", "AGE"],
        constraints: &[
            Constraint::Percentage(35),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    },
];

struct DetailModal {
    title: String,
    lines: Vec<String>,
    scroll: usize,
    visible_height: usize,
}

impl DetailModal {
    fn scroll_down(&mut self) {
        let max = self.lines.len().saturating_sub(self.visible_height);
        if self.scroll < max {
            self.scroll += 1;
        }
    }
    fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }
    fn page_down(&mut self) {
        let max = self.lines.len().saturating_sub(self.visible_height);
        let step = self.visible_height.saturating_sub(1).max(1);
        self.scroll = (self.scroll + step).min(max);
    }
    fn page_up(&mut self) {
        let step = self.visible_height.saturating_sub(1).max(1);
        self.scroll = self.scroll.saturating_sub(step);
    }
    fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }
    fn scroll_to_bottom(&mut self) {
        self.scroll = self.lines.len().saturating_sub(self.visible_height);
    }
}

struct App {
    active_pane: Pane,
    rows: BTreeMap<Pane, Vec<ResourceRow>>,
    namespaces: Vec<String>,
    states: BTreeMap<Pane, TableState>,
    selected_ns_index: usize,
    server_version: String,
    detail: Option<DetailModal>,
}

impl App {
    fn new() -> Self {
        let mut states = BTreeMap::new();
        let mut rows = BTreeMap::new();
        for &pane in Pane::all() {
            let mut state = TableState::default();
            state.select(Some(0));
            states.insert(pane, state);
            rows.insert(pane, vec![]);
        }
        Self {
            active_pane: Pane::Pods,
            rows,
            namespaces: vec!["ALL".to_string()],
            states,
            selected_ns_index: 0,
            server_version: "...".to_string(),
            detail: None,
        }
    }

    fn get_current_ns(&self) -> Option<String> {
        if self.selected_ns_index == 0 {
            None
        } else {
            Some(self.namespaces[self.selected_ns_index].clone())
        }
    }

    fn active_rows_len(&self) -> usize {
        self.rows.get(&self.active_pane).map_or(0, |v| v.len())
    }

    fn selected_row_info(&self) -> Option<(Pane, String, String)> {
        let pane = self.active_pane;
        let idx = self.states.get(&pane)?.selected()?;
        let row = self.rows.get(&pane)?.get(idx)?;
        let name = row.name.clone();
        let ns = row.namespace.clone();
        Some((pane, name, ns))
    }
}

async fn fetch_describe_lines(
    client: &Client,
    pane: Pane,
    name: &str,
    ns: Option<&str>,
) -> Vec<String> {
    match pane {
        Pane::Pods => describe_pod(client, name, ns).await,
        Pane::Services => describe_service(client, name, ns).await,
        Pane::Deployments => describe_deployment(client, name, ns).await,
        Pane::ReplicaSets => describe_replicaset(client, name, ns).await,
        Pane::DaemonSets => describe_daemonset(client, name, ns).await,
        Pane::Jobs => describe_job(client, name, ns).await,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::try_default().await?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new();

    if let Ok(v) = client.apiserver_version().await {
        app.server_version = format!("{}.{}", v.major, v.minor);
    }

    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        let timeout = Duration::from_millis(100);
        if event::poll(timeout.saturating_sub(last_tick.elapsed()))? {
            if let Event::Key(key) = event::read()? {
                if app.detail.is_some() {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                            app.detail = None;
                        }
                        KeyCode::Down => {
                            if let Some(d) = app.detail.as_mut() {
                                d.scroll_down();
                            }
                        }
                        KeyCode::Up => {
                            if let Some(d) = app.detail.as_mut() {
                                d.scroll_up();
                            }
                        }
                        KeyCode::PageDown => {
                            if let Some(d) = app.detail.as_mut() {
                                d.page_down();
                            }
                        }
                        KeyCode::PageUp => {
                            if let Some(d) = app.detail.as_mut() {
                                d.page_up();
                            }
                        }
                        KeyCode::End => {
                            if let Some(d) = app.detail.as_mut() {
                                d.scroll_to_bottom();
                            }
                        }
                        KeyCode::Home => {
                            if let Some(d) = app.detail.as_mut() {
                                d.scroll_to_top();
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => break,

                    KeyCode::Enter => {
                        if let Some((pane, name, row_ns)) = app.selected_row_info() {
                            let ns = if row_ns.is_empty() {
                                None
                            } else {
                                Some(row_ns)
                            };
                            let cfg = PANE_CONFIGS.iter().find(|c| c.pane == pane).unwrap();

                            app.detail = Some(DetailModal {
                                title: format!(" ✦ {} — {} ", cfg.title, name),
                                lines: vec!["  Loading...".into()],
                                scroll: 0,
                                visible_height: 0,
                            });
                            terminal.draw(|f| ui(f, &mut app))?;

                            let lines =
                                fetch_describe_lines(&client, pane, &name, ns.as_deref()).await;

                            app.detail = Some(DetailModal {
                                title: format!(" ✦ {} — {} ", cfg.title, name),
                                lines,
                                scroll: 0,
                                visible_height: 0,
                            });
                        }
                    }

                    KeyCode::Tab => {
                        let idx = Pane::all()
                            .iter()
                            .position(|&p| p == app.active_pane)
                            .unwrap_or(0);
                        app.active_pane = Pane::all()[(idx + 1) % Pane::all().len()];
                    }
                    KeyCode::BackTab => {
                        let idx = Pane::all()
                            .iter()
                            .position(|&p| p == app.active_pane)
                            .unwrap_or(0);
                        app.active_pane =
                            Pane::all()[(idx + Pane::all().len() - 1) % Pane::all().len()];
                    }

                    KeyCode::Down => {
                        let len = app.active_rows_len();
                        let state = app.states.get_mut(&app.active_pane).unwrap();
                        let i = state
                            .selected()
                            .map_or(0, |i| (i + 1).min(len.saturating_sub(1)));
                        state.select(Some(i));
                    }
                    KeyCode::Up => {
                        let state = app.states.get_mut(&app.active_pane).unwrap();
                        let i = state.selected().map_or(0, |i| i.saturating_sub(1));
                        state.select(Some(i));
                    }

                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        let idx = c.to_digit(10).unwrap() as usize;
                        if idx < app.namespaces.len() {
                            app.selected_ns_index = idx;
                        }
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= Duration::from_millis(TICKS_DELAY.into()) {
            app.namespaces = std::iter::once("ALL".to_string())
                .chain(
                    fetch_cluster_resources::<Namespace>(&client)
                        .await
                        .into_iter()
                        .map(|r| r.name),
                )
                .collect();

            let ns = app.get_current_ns();
            *app.rows.get_mut(&Pane::Pods).unwrap() = fetch_resources::<Pod>(&client, &ns).await;
            *app.rows.get_mut(&Pane::Services).unwrap() =
                fetch_resources::<Service>(&client, &ns).await;
            *app.rows.get_mut(&Pane::Deployments).unwrap() =
                fetch_resources::<Deployment>(&client, &ns).await;
            *app.rows.get_mut(&Pane::ReplicaSets).unwrap() =
                fetch_resources::<ReplicaSet>(&client, &ns).await;
            *app.rows.get_mut(&Pane::DaemonSets).unwrap() =
                fetch_resources::<DaemonSet>(&client, &ns).await;
            *app.rows.get_mut(&Pane::Jobs).unwrap() = fetch_resources::<Job>(&client, &ns).await;

            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

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

fn ui(f: &mut Frame, app: &mut App) {
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
