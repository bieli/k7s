use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet};
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{
    Namespace, PersistentVolume, PersistentVolumeClaim, Pod, Service,
};
use kube::Client;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::{io, time::Duration, time::Instant};

mod resources;
use crate::resources::{fetch_cluster_resources, fetch_resources};

mod describe_resources;
use crate::describe_resources::{
    describe_daemonset, describe_deployment, describe_job, describe_pod, describe_pv, describe_pvc,
    describe_replicaset, describe_service,
};

mod app;
use crate::app::{App, DetailModal, Pane, PANE_CONFIGS, PULLING_DURATION_MS, TICKS_DELAY_MS};
mod ui;
use crate::ui::ui_create;

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
        Pane::Pv => describe_pv(client, name).await,
        Pane::Pvc => describe_pvc(client, name, ns).await,
    }
}

fn handle_modal_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    let d = match app.detail.as_mut() {
        Some(d) => d,
        None => return false,
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
            app.detail = None;
        }
        KeyCode::Down => d.scroll_down(),
        KeyCode::Up => d.scroll_up(),
        KeyCode::PageDown => d.page_down(),
        KeyCode::PageUp => d.page_up(),
        KeyCode::End => d.scroll_to_bottom(),
        KeyCode::Home => d.scroll_to_top(),
        _ => {}
    }
    true
}

fn handle_table_navigation(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
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
            app.active_pane = Pane::all()[(idx + Pane::all().len() - 1) % Pane::all().len()];
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

async fn handle_enter(
    app: &mut App,
    client: &Client,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<()> {
    let (pane, name, row_ns) = match app.selected_row_info() {
        Some(info) => info,
        None => return Ok(()),
    };
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
    terminal.draw(|f| ui_create(f, app))?;

    let lines = fetch_describe_lines(client, pane, &name, ns.as_deref()).await;
    app.detail = Some(DetailModal {
        title: format!(" ✦ {} — {} ", cfg.title, name),
        lines,
        scroll: 0,
        visible_height: 0,
    });
    Ok(())
}

async fn tick_refresh(app: &mut App, client: &Client) {
    app.namespaces = std::iter::once("ALL".to_string())
        .chain(
            fetch_cluster_resources::<Namespace>(client)
                .await
                .into_iter()
                .map(|r| r.name),
        )
        .collect();

    let ns = app.get_current_ns();
    *app.rows.get_mut(&Pane::Pods).unwrap() = fetch_resources::<Pod>(client, &ns).await;
    *app.rows.get_mut(&Pane::Services).unwrap() = fetch_resources::<Service>(client, &ns).await;
    *app.rows.get_mut(&Pane::Deployments).unwrap() =
        fetch_resources::<Deployment>(client, &ns).await;
    *app.rows.get_mut(&Pane::ReplicaSets).unwrap() =
        fetch_resources::<ReplicaSet>(client, &ns).await;
    *app.rows.get_mut(&Pane::DaemonSets).unwrap() = fetch_resources::<DaemonSet>(client, &ns).await;
    *app.rows.get_mut(&Pane::Jobs).unwrap() = fetch_resources::<Job>(client, &ns).await;
    *app.rows.get_mut(&Pane::Pv).unwrap() =
        fetch_cluster_resources::<PersistentVolume>(client).await;
    *app.rows.get_mut(&Pane::Pvc).unwrap() =
        fetch_resources::<PersistentVolumeClaim>(client, &ns).await;
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
        terminal.draw(|f| ui_create(f, &mut app))?;

        let timeout = Duration::from_millis(PULLING_DURATION_MS.into());
        if event::poll(timeout.saturating_sub(last_tick.elapsed()))? {
            if let Event::Key(key) = event::read()? {
                if handle_modal_key(&mut app, key) {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Enter => handle_enter(&mut app, &client, &mut terminal).await?,
                    _ => handle_table_navigation(&mut app, key),
                }
            }
        }

        if last_tick.elapsed() >= Duration::from_millis(TICKS_DELAY_MS.into()) {
            tick_refresh(&mut app, &client).await;
            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
