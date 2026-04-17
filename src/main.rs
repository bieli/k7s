use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};

use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Table, Row, Cell, TableState, Block, Borders},
    Terminal, Frame,
};

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
    states: BTreeMap<Pane, TableState>,
    server_version: String,
}

impl App {
    fn new() -> Self {
        let mut states = BTreeMap::new();
        let mut table_state = TableState::default();
        table_state.select(Some(0));

        states.insert(Pane::Pods, table_state);

        Self {
            active_pane: Pane::Pods,
            pods: vec![
                ResourceRow {
                    name: "nginx-7d8f9d6b7c-abc12".into(),
                    data: vec!["Running".into(), "1/1".into(), "3d".into()],
                },
                ResourceRow {
                    name: "api-server-5f76c9d8f9-xyz99".into(),
                    data: vec!["Running".into(), "1/1".into(), "12h".into()],
                },
                ResourceRow {
                    name: "db-0".into(),
                    data: vec!["Pending".into(), "0/1".into(), "2m".into()],
                },
                ResourceRow {
                    name: "worker-6c7b8d9f-ghijk".into(),
                    data: vec!["CrashLoop".into(), "0/1".into(), "5m".into()],
                },
            ],
            states,
            server_version: "v1.30.0".to_string(),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
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

        if last_tick.elapsed() >= Duration::from_millis(100) {
            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(f.size());

    let header = Line::from(vec![
        Span::styled(
            " k7s - kubernetes dashboard ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("| "),
        Span::styled(
            format!("K8s API: {}", app.server_version),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    f.render_widget(Paragraph::new(header), layout[0]);

    let rows: Vec<Row> = app.pods.iter().map(|p| {
        Row::new(vec![
            Cell::from(p.name.clone()),
            Cell::from(p.data[0].clone()),
            Cell::from(p.data[1].clone()),
            Cell::from(p.data[2].clone()),
        ])
    }).collect();

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
        Row::new(vec!["Name", "Status", "Ready", "Age"])
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().title("Pods").borders(Borders::ALL))
    .highlight_style(Style::default().bg(Color::Blue));

    let state = app.states.get_mut(&Pane::Pods).unwrap();
    f.render_stateful_widget(table, layout[1], state);
}
