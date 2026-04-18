use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet};
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{Namespace, Pod, Service};
use kube::{Client};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};
use std::{collections::BTreeMap, io, time::Duration, time::Instant};

mod resources;
use crate::resources::{fetch_cluster_resources, fetch_resources, ResourceRow};

const APP_HEADER_TITLE: &str = "K7s Kubernetes Resources Viewer by @bieli";
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

struct App {
    active_pane: Pane,
    rows: BTreeMap<Pane, Vec<ResourceRow>>,
    namespaces: Vec<String>,
    states: BTreeMap<Pane, TableState>,
    selected_ns_index: usize,
    server_version: String,
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
                match key.code {
                    KeyCode::Char('q') => break,

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
