use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet};
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{Namespace, Pod, Service};
use kube::{Api, Client};
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

fn opt_str(v: &Option<String>) -> &str {
    v.as_deref().unwrap_or("<none>")
}

fn labels_str(m: Option<&BTreeMap<String, String>>) -> String {
    match m {
        None => "<none>".into(),
        Some(map) if map.is_empty() => "<none>".into(),
        Some(map) => map
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("  "),
    }
}

fn section(lines: &mut Vec<String>, title: &str) {
    lines.push("".into());
    lines.push(format!(
        "━━━ {} {}",
        title,
        "━".repeat(54_usize.saturating_sub(title.len()))
    ));
}

fn field(lines: &mut Vec<String>, label: &str, value: &str) {
    lines.push(format!("  {:<28}  {}", label, value));
}

fn int_or_str(v: &k8s_openapi::apimachinery::pkg::util::intstr::IntOrString) -> String {
    match v {
        k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(n) => n.to_string(),
        k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::String(s) => s.clone(),
    }
}

fn annotations_lines(
    lines: &mut Vec<String>,
    label: &str,
    anns: Option<&std::collections::BTreeMap<String, String>>,
) {
    fn is_json_blob(v: &str) -> bool {
        let t = v.trim();
        (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'))
    }

    fn truncate(v: &str) -> String {
        const MAX: usize = 120;
        if v.len() > MAX {
            format!("{}...", &v[..MAX])
        } else {
            v.to_string()
        }
    }

    match anns {
        None => field(lines, label, "<none>"),
        Some(map) => {
            let visible: Vec<_> = map.iter().filter(|(_, v)| !is_json_blob(v)).collect();
            if visible.is_empty() {
                field(lines, label, "<none>");
            } else {
                for (i, (k, v)) in visible.iter().enumerate() {
                    let display = format!("{}: {}", k, truncate(v));
                    if i == 0 {
                        field(lines, label, &display);
                    } else {
                        lines.push(format!("  {:<28}  {}", "", display));
                    }
                }
            }
        }
    }
}

fn multiline_labels(
    lines: &mut Vec<String>,
    label: &str,
    map: Option<&std::collections::BTreeMap<String, String>>,
) {
    match map {
        None => field(lines, label, "<none>"),
        Some(m) if m.is_empty() => field(lines, label, "<none>"),
        Some(m) => {
            let pairs: Vec<String> = m.iter().map(|(k, v)| format!("{}  =  {}", k, v)).collect();
            field(lines, label, &pairs[0]);
            for p in &pairs[1..] {
                lines.push(format!("  {:<28}  {}", "", p));
            }
        }
    }
}

fn probe_str(p: &k8s_openapi::api::core::v1::Probe) -> String {
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
    let action = if let Some(h) = &p.http_get {
        let port = match &h.port {
            IntOrString::Int(n) => n.to_string(),
            IntOrString::String(s) => s.clone(),
        };
        format!(
            "http-get {}:{}{}",
            h.scheme.as_deref().unwrap_or("HTTP").to_lowercase(),
            port,
            h.path.as_deref().unwrap_or("/")
        )
    } else if let Some(e) = &p.exec {
        format!(
            "exec {}",
            e.command.as_ref().map(|c| c.join(" ")).unwrap_or_default()
        )
    } else if let Some(t) = &p.tcp_socket {
        format!("tcp-socket :{:?}", t.port)
    } else {
        "unknown".into()
    };
    format!(
        "{} delay={}s timeout={}s period={}s #success={} #failure={}",
        action,
        p.initial_delay_seconds.unwrap_or(0),
        p.timeout_seconds.unwrap_or(1),
        p.period_seconds.unwrap_or(10),
        p.success_threshold.unwrap_or(1),
        p.failure_threshold.unwrap_or(3),
    )
}

fn env_var_value(ev: &k8s_openapi::api::core::v1::EnvVar) -> String {
    if let Some(v) = &ev.value {
        return v.clone();
    }
    if let Some(vf) = &ev.value_from {
        if let Some(fr) = &vf.field_ref {
            return format!("({})", fr.field_path);
        }
        if let Some(sr) = &vf.secret_key_ref {
            return format!("secret:{}/{}", sr.name.as_deref().unwrap_or("?"), sr.key);
        }
        if let Some(cr) = &vf.config_map_key_ref {
            return format!("configmap:{}/{}", cr.name.as_deref().unwrap_or("?"), cr.key);
        }
    }
    "<none>".into()
}

fn toleration_str(t: &k8s_openapi::api::core::v1::Toleration) -> String {
    let key = t.key.as_deref().unwrap_or("*");
    let op = t.operator.as_deref().unwrap_or("Equal");
    let effect = t.effect.as_deref().unwrap_or("");
    if let Some(val) = &t.value {
        format!("{}={}: {}", key, val, effect)
    } else {
        format!("{}:{} ({})", key, effect, op)
    }
}

async fn describe_deployment(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
    let api: Api<Deployment> = match ns {
        Some(n) => Api::namespaced(client.clone(), n),
        None => Api::all(client.clone()),
    };
    let mut lines = Vec::new();

    let d = match api.get(name).await {
        Ok(d) => d,
        Err(e) => {
            lines.push(format!("  Error fetching Deployment '{}': {}", name, e));
            return lines;
        }
    };

    let meta = &d.metadata;
    let spec = d.spec.as_ref();
    let status = d.status.as_ref();

    section(&mut lines, "Identity");
    field(&mut lines, "Name", opt_str(&meta.name));
    field(&mut lines, "Namespace", opt_str(&meta.namespace));
    field(
        &mut lines,
        "Created",
        &meta
            .creation_timestamp
            .as_ref()
            .map(|t| t.0.to_rfc2822())
            .unwrap_or_else(|| "<none>".into()),
    );
    multiline_labels(&mut lines, "Labels", meta.labels.as_ref());
    annotations_lines(&mut lines, "Annotations", meta.annotations.as_ref());

    section(&mut lines, "Spec");
    if let Some(s) = spec {
        multiline_labels(&mut lines, "Selector", s.selector.match_labels.as_ref());

        let desired = s.replicas.unwrap_or(1);
        let updated = status.and_then(|st| st.updated_replicas).unwrap_or(0);
        let total = status.and_then(|st| st.replicas).unwrap_or(0);
        let available = status.and_then(|st| st.available_replicas).unwrap_or(0);
        let unavailable = status.and_then(|st| st.unavailable_replicas).unwrap_or(0);
        field(
            &mut lines,
            "Replicas",
            &format!(
                "{} desired | {} updated | {} total | {} available | {} unavailable",
                desired, updated, total, available, unavailable
            ),
        );

        if let Some(strategy) = &s.strategy {
            field(
                &mut lines,
                "StrategyType",
                strategy.type_.as_deref().unwrap_or("<none>"),
            );
            if let Some(ru) = &strategy.rolling_update {
                let max_un = ru
                    .max_unavailable
                    .as_ref()
                    .map(int_or_str)
                    .unwrap_or_else(|| "25%".into());
                let max_sur = ru
                    .max_surge
                    .as_ref()
                    .map(int_or_str)
                    .unwrap_or_else(|| "25%".into());
                field(
                    &mut lines,
                    "RollingUpdateStrategy",
                    &format!("{} max unavailable, {} max surge", max_un, max_sur),
                );
            }
        }
        field(
            &mut lines,
            "MinReadySeconds",
            &s.min_ready_seconds.unwrap_or(0).to_string(),
        );
    }

    section(&mut lines, "Pod Template");
    if let Some(s) = spec {
        multiline_labels(
            &mut lines,
            "  Labels",
            s.template.metadata.as_ref().and_then(|m| m.labels.as_ref()),
        );

        if let Some(pod_spec) = &s.template.spec {
            field(
                &mut lines,
                "  ServiceAccount",
                pod_spec.service_account_name.as_deref().unwrap_or("<none>"),
            );

            for c in &pod_spec.containers {
                lines.push(format!("  Container: {}", c.name));
                field(
                    &mut lines,
                    "    Image",
                    c.image.as_deref().unwrap_or("<none>"),
                );

                let ports = c
                    .ports
                    .as_ref()
                    .map(|ps| {
                        ps.iter()
                            .map(|p| {
                                let proto = p.protocol.as_deref().unwrap_or("TCP");
                                match p.name.as_deref() {
                                    Some(n) => format!("{}/{} ({})", p.container_port, proto, n),
                                    None => format!("{}/{}", p.container_port, proto),
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "<none>".into());
                field(&mut lines, "    Ports", &ports);

                if let Some(args) = &c.args {
                    if !args.is_empty() {
                        field(&mut lines, "    Args", &args[0]);
                        for a in &args[1..] {
                            lines.push(format!("  {:<28}  {}", "", a));
                        }
                    }
                }

                if let Some(env) = &c.env {
                    if !env.is_empty() {
                        let first = &env[0];
                        let val = env_var_value(first);
                        field(&mut lines, "    Env", &format!("{}: {}", first.name, val));
                        for ev in &env[1..] {
                            let val = env_var_value(ev);
                            lines.push(format!("  {:<28}  {}: {}", "", ev.name, val));
                        }
                    }
                } else {
                    field(&mut lines, "    Env", "<none>");
                }

                if let Some(res) = &c.resources {
                    if let Some(req) = &res.requests {
                        let s = req
                            .iter()
                            .map(|(k, v)| format!("{}: {}", k, v.0))
                            .collect::<Vec<_>>()
                            .join("  ");
                        field(&mut lines, "    Requests", &s);
                    }
                    if let Some(lim) = &res.limits {
                        let s = lim
                            .iter()
                            .map(|(k, v)| format!("{}: {}", k, v.0))
                            .collect::<Vec<_>>()
                            .join("  ");
                        field(&mut lines, "    Limits", &s);
                    }
                }

                if let Some(p) = &c.liveness_probe {
                    field(&mut lines, "    Liveness", &probe_str(p));
                }
                if let Some(p) = &c.readiness_probe {
                    field(&mut lines, "    Readiness", &probe_str(p));
                }

                if let Some(mounts) = &c.volume_mounts {
                    if !mounts.is_empty() {
                        let ms = mounts
                            .iter()
                            .map(|m| {
                                format!(
                                    "{}{}",
                                    m.mount_path,
                                    if m.read_only.unwrap_or(false) {
                                        " (ro)"
                                    } else {
                                        ""
                                    }
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        field(&mut lines, "    Mounts", &ms);
                    }
                }
            }

            if let Some(vols) = &pod_spec.volumes {
                lines.push("  Volumes:".into());
                for v in vols {
                    lines.push(format!("    {}", v.name));
                    if let Some(sec) = &v.secret {
                        field(&mut lines, "      Type", "Secret");
                        field(
                            &mut lines,
                            "      SecretName",
                            sec.secret_name.as_deref().unwrap_or("<none>"),
                        );
                    } else if let Some(cm) = &v.config_map {
                        field(&mut lines, "      Type", "ConfigMap");
                        field(
                            &mut lines,
                            "      ConfigMapName",
                            cm.name.as_deref().unwrap_or("<none>"),
                        );
                    } else if v.empty_dir.is_some() {
                        field(&mut lines, "      Type", "EmptyDir");
                    } else {
                        field(&mut lines, "      Type", "other");
                    }
                }
            }

            multiline_labels(
                &mut lines,
                "  Node-Selectors",
                pod_spec.node_selector.as_ref(),
            );
            if let Some(tols) = &pod_spec.tolerations {
                if tols.is_empty() {
                    field(&mut lines, "  Tolerations", "<none>");
                } else {
                    let first = toleration_str(&tols[0]);
                    field(&mut lines, "  Tolerations", &first);
                    for t in &tols[1..] {
                        lines.push(format!("  {:<28}  {}", "", toleration_str(t)));
                    }
                }
            }
        }
    }

    section(&mut lines, "Conditions");
    if let Some(st) = status {
        if let Some(conds) = &st.conditions {
            lines.push(format!("  {:<20}  {:<8}  {}", "Type", "Status", "Reason"));
            lines.push(format!("  {:<20}  {:<8}  {}", "────", "──────", "──────"));
            for c in conds {
                lines.push(format!(
                    "  {:<20}  {:<8}  {}",
                    c.type_,
                    c.status,
                    c.reason.as_deref().unwrap_or("")
                ));
            }
        }
    }

    section(&mut lines, "Hints");
    lines.push(format!(
        "  kubectl describe deployment/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    lines.push(format!(
        "  kubectl get deployment/{} -n {} -o yaml",
        name,
        ns.unwrap_or("default")
    ));
    lines.push("".into());
    lines.push("  Esc / q — close   ↑ ↓ PgDn PgUp Home End — navigate".into());
    lines
}

async fn describe_pod(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
    let api: Api<Pod> = match ns {
        Some(n) => Api::namespaced(client.clone(), n),
        None => Api::all(client.clone()),
    };
    let mut lines = Vec::new();

    let p = match api.get(name).await {
        Ok(p) => p,
        Err(e) => {
            lines.push(format!("  Error fetching Pod '{}': {}", name, e));
            return lines;
        }
    };

    let meta = &p.metadata;
    let spec = p.spec.as_ref();
    let status = p.status.as_ref();

    section(&mut lines, "Identity");
    field(&mut lines, "Name", opt_str(&meta.name));
    field(&mut lines, "Namespace", opt_str(&meta.namespace));
    field(
        &mut lines,
        "Created",
        &meta
            .creation_timestamp
            .as_ref()
            .map(|t| t.0.to_rfc2822())
            .unwrap_or_else(|| "<none>".into()),
    );
    multiline_labels(&mut lines, "Labels", meta.labels.as_ref());
    field(
        &mut lines,
        "Node",
        spec.and_then(|s| s.node_name.as_deref())
            .unwrap_or("<none>"),
    );
    field(
        &mut lines,
        "ServiceAccount",
        spec.and_then(|s| s.service_account_name.as_deref())
            .unwrap_or("<none>"),
    );

    section(&mut lines, "Status");
    field(
        &mut lines,
        "Phase",
        status.and_then(|s| s.phase.as_deref()).unwrap_or("<none>"),
    );
    field(
        &mut lines,
        "Pod IP",
        status.and_then(|s| s.pod_ip.as_deref()).unwrap_or("<none>"),
    );
    field(
        &mut lines,
        "Host IP",
        status
            .and_then(|s| s.host_ip.as_deref())
            .unwrap_or("<none>"),
    );

    section(&mut lines, "Containers");
    if let Some(s) = spec {
        for c in &s.containers {
            lines.push(format!("  Container: {}", c.name));
            field(
                &mut lines,
                "    Image",
                c.image.as_deref().unwrap_or("<none>"),
            );
            let ports = c
                .ports
                .as_ref()
                .map(|ps| {
                    ps.iter()
                        .map(|p| {
                            format!(
                                "{}/{}",
                                p.container_port,
                                p.protocol.as_deref().unwrap_or("TCP")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| "<none>".into());
            field(&mut lines, "    Ports", &ports);
            let limits = c
                .resources
                .as_ref()
                .and_then(|r| r.limits.as_ref())
                .map(|l| {
                    l.iter()
                        .map(|(k, v)| format!("{}={}", k, v.0))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| "<none>".into());
            field(&mut lines, "    Limits", &limits);
        }
    }

    section(&mut lines, "Container Statuses");
    if let Some(cs_list) = status.and_then(|s| s.container_statuses.as_ref()) {
        for cs in cs_list {
            lines.push(format!("  Container: {}", cs.name));
            field(&mut lines, "    Ready", &cs.ready.to_string());
            field(
                &mut lines,
                "    Restart count",
                &cs.restart_count.to_string(),
            );
            field(&mut lines, "    Image", &cs.image);
            if let Some(state) = &cs.state {
                if let Some(r) = &state.running {
                    field(
                        &mut lines,
                        "    State",
                        &format!(
                            "Running since {}",
                            r.started_at
                                .as_ref()
                                .map(|t| t.0.to_rfc2822())
                                .unwrap_or_else(|| "?".into())
                        ),
                    );
                } else if let Some(w) = &state.waiting {
                    field(
                        &mut lines,
                        "    State",
                        &format!("Waiting — {}", w.reason.as_deref().unwrap_or("?")),
                    );
                } else if let Some(t) = &state.terminated {
                    field(
                        &mut lines,
                        "    State",
                        &format!(
                            "Terminated — exit {} ({})",
                            t.exit_code,
                            t.reason.as_deref().unwrap_or("?")
                        ),
                    );
                }
            }
        }
    }

    section(&mut lines, "Conditions");
    if let Some(conds) = status.and_then(|s| s.conditions.as_ref()) {
        lines.push(format!("  {:<24}  {}", "Type", "Status"));
        lines.push(format!("  {:<24}  {}", "────", "──────"));
        for c in conds {
            lines.push(format!("  {:<24}  {}", c.type_, c.status));
        }
    }

    section(&mut lines, "Hints");
    lines.push(format!(
        "  kubectl describe pod/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    lines.push(format!(
        "  kubectl logs {} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    lines.push("".into());
    lines.push("  Esc / q — close   ↑ ↓ — scroll".into());
    lines
}

async fn describe_service(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
    let api: Api<Service> = match ns {
        Some(n) => Api::namespaced(client.clone(), n),
        None => Api::all(client.clone()),
    };
    let mut lines = Vec::new();

    let svc = match api.get(name).await {
        Ok(s) => s,
        Err(e) => {
            lines.push(format!("  Error fetching Service '{}': {}", name, e));
            return lines;
        }
    };

    let meta = &svc.metadata;
    let spec = svc.spec.as_ref();

    section(&mut lines, "Identity");
    field(&mut lines, "Name", opt_str(&meta.name));
    field(&mut lines, "Namespace", opt_str(&meta.namespace));
    field(
        &mut lines,
        "Created",
        &meta
            .creation_timestamp
            .as_ref()
            .map(|t| t.0.to_rfc2822())
            .unwrap_or_else(|| "<none>".into()),
    );
    multiline_labels(&mut lines, "Labels", meta.labels.as_ref());
    annotations_lines(&mut lines, "Annotations", meta.annotations.as_ref());

    section(&mut lines, "Spec");
    if let Some(s) = spec {
        field(&mut lines, "Type", s.type_.as_deref().unwrap_or("<none>"));
        field(
            &mut lines,
            "ClusterIP",
            s.cluster_ip.as_deref().unwrap_or("<none>"),
        );
        multiline_labels(&mut lines, "Selector", s.selector.as_ref());

        if let Some(ps) = &s.ports {
            if ps.is_empty() {
                field(&mut lines, "Ports", "<none>");
            } else {
                for (i, p) in ps.iter().enumerate() {
                    let proto = p.protocol.as_deref().unwrap_or("TCP");
                    let name_part = p
                        .name
                        .as_ref()
                        .map(|n| format!(" ({})", n))
                        .unwrap_or_default();
                    let target = p
                        .target_port
                        .as_ref()
                        .map(int_or_str)
                        .unwrap_or_else(|| p.port.to_string());
                    let node_part = p
                        .node_port
                        .map(|np| format!("  NodePort: {}", np))
                        .unwrap_or_default();
                    let line = format!(
                        "{}/{}{} -> {}{}",
                        p.port, proto, name_part, target, node_part
                    );
                    if i == 0 {
                        field(&mut lines, "Ports", &line);
                    } else {
                        lines.push(format!("  {:<28}  {}", "", line));
                    }
                }
            }
        } else {
            field(&mut lines, "Ports", "<none>");
        }

        let external_ips = s
            .external_ips
            .as_ref()
            .map(|v: &Vec<String>| v.join(", "))
            .unwrap_or_else(|| "<none>".into());
        field(&mut lines, "External IPs", &external_ips);

        if let Some(status) = &svc.status {
            if let Some(lb) = &status.load_balancer {
                if let Some(ingresses) = &lb.ingress {
                    if !ingresses.is_empty() {
                        let ips = ingresses
                            .iter()
                            .filter_map(|i| i.ip.clone().or_else(|| i.hostname.clone()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        field(&mut lines, "LoadBalancer Ingress", &ips);
                    }
                }
            }
        }

        if let Some(families) = &s.ip_families {
            field(&mut lines, "IP Families", &families.join(", "));
        }
        if let Some(policy) = &s.ip_family_policy {
            field(&mut lines, "IP Family Policy", policy);
        }
    }

    section(&mut lines, "Hints");
    lines.push(format!(
        "  kubectl describe service/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    lines.push(format!(
        "  kubectl get endpoints/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    lines.push("".into());
    lines.push("  Esc / q — close   ↑ ↓ PgDn PgUp Home End — navigate".into());
    lines
}

async fn describe_replicaset(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
    let api: Api<ReplicaSet> = match ns {
        Some(n) => Api::namespaced(client.clone(), n),
        None => Api::all(client.clone()),
    };
    let mut lines = Vec::new();

    let rs = match api.get(name).await {
        Ok(r) => r,
        Err(e) => {
            lines.push(format!("  Error fetching ReplicaSet '{}': {}", name, e));
            return lines;
        }
    };

    let meta = &rs.metadata;
    let spec = rs.spec.as_ref();
    let status = rs.status.as_ref();

    section(&mut lines, "Identity");
    field(&mut lines, "Name", opt_str(&meta.name));
    field(&mut lines, "Namespace", opt_str(&meta.namespace));
    field(
        &mut lines,
        "Created",
        &meta
            .creation_timestamp
            .as_ref()
            .map(|t| t.0.to_rfc2822())
            .unwrap_or_else(|| "<none>".into()),
    );
    multiline_labels(&mut lines, "Labels", meta.labels.as_ref());
    let owner = meta
        .owner_references
        .as_ref()
        .map(|o| {
            o.iter()
                .map(|r| format!("{}/{}", r.kind, r.name))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "<none>".into());
    field(&mut lines, "Owned by", &owner);

    section(&mut lines, "Replicas");
    field(
        &mut lines,
        "Desired",
        &spec.and_then(|s| s.replicas).unwrap_or(0).to_string(),
    );
    field(
        &mut lines,
        "Current",
        &status.map(|s| s.replicas).unwrap_or(0).to_string(),
    );
    field(
        &mut lines,
        "Ready",
        &status
            .and_then(|s| s.ready_replicas)
            .unwrap_or(0)
            .to_string(),
    );
    field(
        &mut lines,
        "Available",
        &status
            .and_then(|s| s.available_replicas)
            .unwrap_or(0)
            .to_string(),
    );

    section(&mut lines, "Pod Template");
    if let Some(s) = spec {
        multiline_labels(
            &mut lines,
            "  Labels",
            s.template
                .as_ref()
                .and_then(|t| t.metadata.as_ref())
                .and_then(|m| m.labels.as_ref()),
        );
        if let Some(pod_spec) = s.template.as_ref().and_then(|t| t.spec.as_ref()) {
            for c in &pod_spec.containers {
                lines.push(format!("  Container: {}", c.name));
                field(
                    &mut lines,
                    "    Image",
                    c.image.as_deref().unwrap_or("<none>"),
                );
            }
        }
    }

    section(&mut lines, "Hints");
    lines.push(format!(
        "  kubectl describe replicaset/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    lines.push("".into());
    lines.push("  Esc / q — close   ↑ ↓ — scroll".into());
    lines
}

async fn describe_daemonset(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
    let api: Api<DaemonSet> = match ns {
        Some(n) => Api::namespaced(client.clone(), n),
        None => Api::all(client.clone()),
    };
    let mut lines = Vec::new();

    let ds = match api.get(name).await {
        Ok(d) => d,
        Err(e) => {
            lines.push(format!("  Error fetching DaemonSet '{}': {}", name, e));
            return lines;
        }
    };

    let meta = &ds.metadata;
    let spec = ds.spec.as_ref();
    let status = ds.status.as_ref();

    section(&mut lines, "Identity");
    field(&mut lines, "Name", opt_str(&meta.name));
    field(&mut lines, "Namespace", opt_str(&meta.namespace));
    field(
        &mut lines,
        "Created",
        &meta
            .creation_timestamp
            .as_ref()
            .map(|t| t.0.to_rfc2822())
            .unwrap_or_else(|| "<none>".into()),
    );
    multiline_labels(&mut lines, "Labels", meta.labels.as_ref());

    section(&mut lines, "Status");
    field(
        &mut lines,
        "Desired",
        &status
            .map(|s| s.desired_number_scheduled)
            .unwrap_or(0)
            .to_string(),
    );
    field(
        &mut lines,
        "Current",
        &status
            .map(|s| s.current_number_scheduled)
            .unwrap_or(0)
            .to_string(),
    );
    field(
        &mut lines,
        "Ready",
        &status.map(|s| s.number_ready).unwrap_or(0).to_string(),
    );
    field(
        &mut lines,
        "Available",
        &status
            .and_then(|s| s.number_available)
            .unwrap_or(0)
            .to_string(),
    );
    field(
        &mut lines,
        "Unavailable",
        &status
            .and_then(|s| s.number_unavailable)
            .unwrap_or(0)
            .to_string(),
    );

    section(&mut lines, "Pod Template");
    if let Some(s) = spec {
        if let Some(pod_spec) = &s.template.spec {
            for c in &pod_spec.containers {
                lines.push(format!("  Container: {}", c.name));
                field(
                    &mut lines,
                    "    Image",
                    c.image.as_deref().unwrap_or("<none>"),
                );
            }
            multiline_labels(
                &mut lines,
                "  Node Selector",
                pod_spec.node_selector.as_ref(),
            );
        }
    }

    section(&mut lines, "Hints");
    lines.push(format!(
        "  kubectl describe daemonset/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    lines.push("".into());
    lines.push("  Esc / q — close   ↑ ↓ — scroll".into());
    lines
}

async fn describe_job(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
    let api: Api<Job> = match ns {
        Some(n) => Api::namespaced(client.clone(), n),
        None => Api::all(client.clone()),
    };
    let mut lines = Vec::new();

    let j = match api.get(name).await {
        Ok(j) => j,
        Err(e) => {
            lines.push(format!("  Error fetching Job '{}': {}", name, e));
            return lines;
        }
    };

    let meta = &j.metadata;
    let spec = j.spec.as_ref();
    let status = j.status.as_ref();

    section(&mut lines, "Identity");
    field(&mut lines, "Name", opt_str(&meta.name));
    field(&mut lines, "Namespace", opt_str(&meta.namespace));
    field(
        &mut lines,
        "Created",
        &meta
            .creation_timestamp
            .as_ref()
            .map(|t| t.0.to_rfc2822())
            .unwrap_or_else(|| "<none>".into()),
    );
    multiline_labels(&mut lines, "Labels", meta.labels.as_ref());

    section(&mut lines, "Spec");
    field(
        &mut lines,
        "Completions",
        &spec.and_then(|s| s.completions).unwrap_or(1).to_string(),
    );
    field(
        &mut lines,
        "Parallelism",
        &spec.and_then(|s| s.parallelism).unwrap_or(1).to_string(),
    );
    field(
        &mut lines,
        "BackoffLimit",
        &spec.and_then(|s| s.backoff_limit).unwrap_or(6).to_string(),
    );

    section(&mut lines, "Status");
    field(
        &mut lines,
        "Active",
        &status.and_then(|s| s.active).unwrap_or(0).to_string(),
    );
    field(
        &mut lines,
        "Succeeded",
        &status.and_then(|s| s.succeeded).unwrap_or(0).to_string(),
    );
    field(
        &mut lines,
        "Failed",
        &status.and_then(|s| s.failed).unwrap_or(0).to_string(),
    );
    field(
        &mut lines,
        "Start time",
        &status
            .and_then(|s| s.start_time.as_ref())
            .map(|t| t.0.to_rfc2822())
            .unwrap_or_else(|| "<none>".into()),
    );
    field(
        &mut lines,
        "End time",
        &status
            .and_then(|s| s.completion_time.as_ref())
            .map(|t| t.0.to_rfc2822())
            .unwrap_or_else(|| "<none>".into()),
    );

    section(&mut lines, "Conditions");
    if let Some(conds) = status.and_then(|s| s.conditions.as_ref()) {
        lines.push(format!("  {:<20}  {:<8}  {}", "Type", "Status", "Message"));
        lines.push(format!("  {:<20}  {:<8}  {}", "────", "──────", "───────"));
        for c in conds {
            lines.push(format!(
                "  {:<20}  {:<8}  {}",
                c.type_,
                c.status,
                c.message.as_deref().unwrap_or("")
            ));
        }
    }

    section(&mut lines, "Hints");
    lines.push(format!(
        "  kubectl describe job/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    lines.push("".into());
    lines.push("  Esc / q — close   ↑ ↓ — scroll".into());
    lines
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
