use crate::resources::ResourceRow;
use ratatui::prelude::Constraint;
use ratatui::widgets::TableState;
use std::collections::BTreeMap;

pub const APP_HEADER_TITLE: &str = concat!(
    "Kubernetes Resources Viewer by @bieli v",
    env!("CARGO_PKG_VERSION")
);
pub const APP_HEADER_TITLE_LEFT: &str = "--- [ ";
pub const APP_HEADER_TITLE_RIGHT: &str = " ] ---";
pub const APP_HEADER_TITLE_K8S_VER: &str = "| K8s API: v";
pub const TICKS_DELAY: u32 = 1000;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pane {
    Pods,
    Services,
    Deployments,
    ReplicaSets,
    DaemonSets,
    Jobs,
}

impl Pane {
    pub fn all() -> &'static [Pane] {
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

pub struct PaneConfig {
    pub pane: Pane,
    pub title: &'static str,
    pub headers: &'static [&'static str],
    pub constraints: &'static [Constraint],
}

pub const PANE_CONFIGS: &[PaneConfig] = &[
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

pub struct DetailModal {
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
    pub visible_height: usize,
}

impl DetailModal {
    pub fn scroll_down(&mut self) {
        let max = self.lines.len().saturating_sub(self.visible_height);
        if self.scroll < max {
            self.scroll += 1;
        }
    }
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }
    pub fn page_down(&mut self) {
        let max = self.lines.len().saturating_sub(self.visible_height);
        let step = self.visible_height.saturating_sub(1).max(1);
        self.scroll = (self.scroll + step).min(max);
    }
    pub fn page_up(&mut self) {
        let step = self.visible_height.saturating_sub(1).max(1);
        self.scroll = self.scroll.saturating_sub(step);
    }
    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }
    pub fn scroll_to_bottom(&mut self) {
        self.scroll = self.lines.len().saturating_sub(self.visible_height);
    }
}

pub struct App {
    pub active_pane: Pane,
    pub rows: BTreeMap<Pane, Vec<ResourceRow>>,
    pub namespaces: Vec<String>,
    pub states: BTreeMap<Pane, TableState>,
    pub selected_ns_index: usize,
    pub server_version: String,
    pub detail: Option<DetailModal>,
}

impl App {
    pub fn new() -> Self {
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

    pub fn get_current_ns(&self) -> Option<String> {
        if self.selected_ns_index == 0 {
            None
        } else {
            Some(self.namespaces[self.selected_ns_index].clone())
        }
    }

    pub fn active_rows_len(&self) -> usize {
        self.rows.get(&self.active_pane).map_or(0, |v| v.len())
    }

    pub fn selected_row_info(&self) -> Option<(Pane, String, String)> {
        let pane = self.active_pane;
        let idx = self.states.get(&pane)?.selected()?;
        let row = self.rows.get(&pane)?.get(idx)?;
        let name = row.name.clone();
        let ns = row.namespace.clone();
        Some((pane, name, ns))
    }
}
