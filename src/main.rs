use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use k8s_openapi::api::core::v1::{Namespace, Pod};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{api::ListParams, Api, Client};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};
use std::{collections::BTreeMap, io, time::Duration, time::Instant};

const APP_HEADER_TITLE: &str = "K7s Kubernetes Resources Viewer";
const APP_HEADER_TITLE_LEFT: &str = "--- [ ";
const APP_HEADER_TITLE_RIGHT: &str = " ] ---";
const APP_HEADER_TITLE_K8S_VER: &str = "| K8s API: v";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Pane {
    Pods,
}

struct ResourceRow {
    name: String,
    data: Vec<String>,
}

struct App {
    active_pane: Pane,
    pods: Vec<ResourceRow>,
    namespaces: Vec<String>,
    states: BTreeMap<Pane, TableState>,
    server_version: String,
    selected_ns_index: usize,
}

impl App {
    fn new() -> Self {
        let mut states = BTreeMap::new();
        let mut table_state = TableState::default();
        table_state.select(Some(0));

        states.insert(Pane::Pods, table_state);

        Self {
            active_pane: Pane::Pods,
            pods: vec![],
            namespaces: vec!["ALL".to_string()],
            states,
            server_version: "...".to_string(),
            selected_ns_index: 0,
        }
    }

    fn get_current_ns(&self) -> Option<String> {
        if self.selected_ns_index == 0 {
            None
        } else {
            Some(self.namespaces[self.selected_ns_index].clone())
        }
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
        let elapsed = last_tick.elapsed();

        if event::poll(timeout.saturating_sub(elapsed))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Down => {
                        let state = app.states.get_mut(&Pane::Pods).unwrap();
                        let i = match state.selected() {
                            Some(i) => (i + 1).min(app.pods.len() - 1),
                            None => 0,
                        };
                        state.select(Some(i));
                    }
                    KeyCode::Up => {
                        let state = app.states.get_mut(&Pane::Pods).unwrap();
                        let i = match state.selected() {
                            Some(i) => i.saturating_sub(1),
                            None => 0,
                        };
                        state.select(Some(i));
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= Duration::from_millis(1000) {
            if let Ok(ns_list) = Api::<Namespace>::all(client.clone())
                .list(&ListParams::default())
                .await
            {
                app.namespaces = std::iter::once("ALL".to_string())
                    .chain(ns_list.items.into_iter().filter_map(|n| n.metadata.name))
                    .collect();
                let t = app.get_current_ns();
                app.pods = fetch_pods(&client, &t).await;
            }

            last_tick = std::time::Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

async fn fetch_pods(client: &Client, ns: &Option<String>) -> Vec<ResourceRow> {
    let api: Api<Pod> = ns.as_ref().map_or(Api::all(client.clone()), |n| {
        Api::namespaced(client.clone(), n)
    });
    api.list(&ListParams::default())
        .await
        .map(|l| {
            l.items
                .into_iter()
                .map(|p| {
                    let status = p.status.as_ref();
                    let ready = status
                        .and_then(|s| s.container_statuses.as_ref())
                        .map(|cs| format!("{}/{}", cs.iter().filter(|c| c.ready).count(), cs.len()))
                        .unwrap_or_else(|| "0/0".into());
                    let phase = status
                        .and_then(|s| s.phase.clone())
                        .unwrap_or_else(|| "Unknown".into());
                    let restarts = status
                        .and_then(|s| s.container_statuses.as_ref())
                        .map(|cs| cs.iter().map(|c| c.restart_count).sum::<i32>().to_string())
                        .unwrap_or_else(|| "0".into());
                    ResourceRow {
                        name: p.metadata.name.clone().unwrap_or_default(),
                        data: vec![ready, phase, restarts, get_age(&p.metadata)],
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn get_age(meta: &ObjectMeta) -> String {
    let now = chrono::Utc::now();
    if let Some(creation) = meta.creation_timestamp.as_ref() {
        let duration = now.signed_duration_since(creation.0);
        if duration.num_days() > 0 {
            format!("{}d", duration.num_days())
        } else if duration.num_hours() > 0 {
            format!("{}h", duration.num_hours())
        } else {
            format!("{}m", duration.num_minutes())
        }
    } else {
        "-".into()
    }
}

fn ui_header(f: &mut Frame, layout: &Rect, app: &mut App) {

    let header_paragraph = Paragraph::new(Line::from(vec![
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

    f.render_widget(header_paragraph, *layout);
}

fn ui(f: &mut Frame, app: &mut App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(f.size());

    ui_header(f, &layout[0], app);

    let ns_spans: Vec<Span> = app
        .namespaces
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let s = if i == app.selected_ns_index {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            Span::styled(format!("[{}] {}  ", i, n), s)
        })
        .collect();
    f.render_widget(
        Paragraph::new(Line::from(ns_spans)).block(
            Block::default()
                .title(" Namespaces (0-9) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        ),
        layout[1],
    );

    let rows: Vec<Row> = app
        .pods
        .iter()
        .map(|p| {
            Row::new(vec![
                Cell::from(p.name.clone()),
                Cell::from(p.data[0].clone()),
                Cell::from(p.data[1].clone()),
                Cell::from(p.data[2].clone()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(50),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    )
    .header(
        Row::new(vec!["Name", "Status", "Ready", "Age"]).style(Style::default().fg(Color::Blue)),
    )
    .block(Block::default().title("Pods").borders(Borders::ALL))
    .highlight_style(Style::default().bg(Color::Blue));

    let state = app.states.get_mut(&Pane::Pods).unwrap();
    f.render_stateful_widget(table, layout[2], state);
}
