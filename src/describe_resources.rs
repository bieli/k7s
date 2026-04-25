use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet};
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStatus, PersistentVolume, PersistentVolumeClaim, Pod, PodSpec,
    PodStatus, Service,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{Api, Client};
use std::collections::BTreeMap;

fn opt_str(v: &Option<String>) -> &str {
    v.as_deref().unwrap_or("<none>")
}

fn is_json_blob(v: &str) -> bool {
    let t = v.trim();
    (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'))
}

fn truncate(v: &str) -> String {
    const MAX: usize = 120;
    v.get(..MAX)
        .map(|s| format!("{}…", s))
        .unwrap_or_else(|| v.to_string())
}

fn annotation_display(label: &str, key: &str, value: &str, first: bool) -> String {
    let display = format!("{}: {}", key, truncate(value));
    if first {
        format!("  {:<28}  {}", label, display)
    } else {
        format!("  {:<28}  {}", "", display)
    }
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
    anns: Option<&BTreeMap<String, String>>,
) {
    let visible: Vec<_> = match anns {
        None => {
            field(lines, label, "<none>");
            return;
        }
        Some(map) => map.iter().filter(|(_, v)| !is_json_blob(v)).collect(),
    };

    if visible.is_empty() {
        field(lines, label, "<none>");
        return;
    }

    lines.extend(
        visible
            .iter()
            .enumerate()
            .map(|(i, (k, v))| annotation_display(label, k, v, i == 0)),
    );
}

fn section(lines: &mut Vec<String>, title: &str, is_bold: bool) {
    let mut line_char: &str = "─";
    if is_bold {
        line_char = "━";
    }
    lines.push("".into());
    lines.push(format!(
        "{} {} {}",
        line_char.repeat(3),
        title,
        line_char.repeat(54_usize.saturating_sub(title.len()))
    ));
}

fn field(lines: &mut Vec<String>, label: &str, value: &str) {
    lines.push(format!("  {:<28}  {}", label, value));
}

fn deployment_section_spec(
    lines: &mut Vec<String>,
    spec: &k8s_openapi::api::apps::v1::DeploymentSpec,
    status: Option<&k8s_openapi::api::apps::v1::DeploymentStatus>,
) {
    section(lines, "Spec", true);
    multiline_labels(lines, "Selector", spec.selector.match_labels.as_ref());

    let desired = spec.replicas.unwrap_or(1);
    let updated = status.and_then(|s| s.updated_replicas).unwrap_or(0);
    let total = status.and_then(|s| s.replicas).unwrap_or(0);
    let available = status.and_then(|s| s.available_replicas).unwrap_or(0);
    let unavailable = status.and_then(|s| s.unavailable_replicas).unwrap_or(0);
    field(
        lines,
        "Replicas",
        &format!(
            "{} desired | {} updated | {} total | {} available | {} unavailable",
            desired, updated, total, available, unavailable
        ),
    );

    if let Some(strategy) = &spec.strategy {
        field(
            lines,
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
                lines,
                "RollingUpdateStrategy",
                &format!("{} max unavailable, {} max surge", max_un, max_sur),
            );
        }
    }
    field(
        lines,
        "MinReadySeconds",
        &spec.min_ready_seconds.unwrap_or(0).to_string(),
    );
}

fn format_container_ports(c: &k8s_openapi::api::core::v1::Container) -> String {
    c.ports
        .as_ref()
        .map(|ps| {
            ps.iter()
                .map(|p| {
                    let proto = p.protocol.as_deref().unwrap_or("TCP");
                    p.name
                        .as_deref()
                        .map(|n| format!("{}/{} ({})", p.container_port, proto, n))
                        .unwrap_or_else(|| format!("{}/{}", p.container_port, proto))
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "<none>".into())
}

fn push_multiline(lines: &mut Vec<String>, label: &str, entries: &[String]) {
    match entries.split_first() {
        None => field(lines, label, "<none>"),
        Some((first, rest)) => {
            field(lines, label, first);
            rest.iter()
                .for_each(|e| lines.push(format!("  {:<28}  {}", "", e)));
        }
    }
}

fn container_args(c: &k8s_openapi::api::core::v1::Container) -> Vec<String> {
    c.args.as_deref().unwrap_or(&[]).to_vec()
}

fn container_env_entries(c: &k8s_openapi::api::core::v1::Container) -> Vec<String> {
    c.env
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|ev| format!("{}: {}", ev.name, env_var_value(ev)))
        .collect()
}

fn format_resource_map(
    map: &std::collections::BTreeMap<
        String,
        k8s_openapi::apimachinery::pkg::api::resource::Quantity,
    >,
) -> String {
    map.iter()
        .map(|(k, v)| format!("{}: {}", k, v.0))
        .collect::<Vec<_>>()
        .join("  ")
}

fn container_mounts(c: &k8s_openapi::api::core::v1::Container) -> String {
    let ms: Vec<_> = c
        .volume_mounts
        .as_deref()
        .unwrap_or(&[])
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
        .collect();
    if ms.is_empty() {
        "<none>".into()
    } else {
        ms.join(", ")
    }
}

fn deployment_container_lines(lines: &mut Vec<String>, c: &k8s_openapi::api::core::v1::Container) {
    lines.push(format!("  Container: {}", c.name));
    field(lines, "    Image", c.image.as_deref().unwrap_or("<none>"));
    field(lines, "    Ports", &format_container_ports(c));

    push_multiline(lines, "    Args", &container_args(c));
    push_multiline(lines, "    Env", &container_env_entries(c));

    let res = c.resources.as_ref();
    res.and_then(|r| r.requests.as_ref())
        .map(|r| field(lines, "    Requests", &format_resource_map(r)));
    res.and_then(|r| r.limits.as_ref())
        .map(|l| field(lines, "    Limits", &format_resource_map(l)));

    c.liveness_probe
        .as_ref()
        .map(|p| field(lines, "    Liveness", &probe_str(p)));
    c.readiness_probe
        .as_ref()
        .map(|p| field(lines, "    Readiness", &probe_str(p)));

    field(lines, "    Mounts", &container_mounts(c));
}

fn deployment_volume_lines(lines: &mut Vec<String>, vols: &[k8s_openapi::api::core::v1::Volume]) {
    lines.push("  Volumes:".into());
    for v in vols {
        lines.push(format!("    {}", v.name));
        let (type_label, name_val) = if let Some(sec) = &v.secret {
            (
                "Secret",
                sec.secret_name.as_deref().unwrap_or("<none>").to_string(),
            )
        } else if let Some(cm) = &v.config_map {
            (
                "ConfigMap",
                cm.name.as_deref().unwrap_or("<none>").to_string(),
            )
        } else if v.empty_dir.is_some() {
            ("EmptyDir", String::new())
        } else {
            ("other", String::new())
        };
        field(lines, "      Type", type_label);
        if !name_val.is_empty() {
            field(lines, "      Name", &name_val);
        }
    }
}

fn deployment_section_pod_template(
    lines: &mut Vec<String>,
    spec: &k8s_openapi::api::apps::v1::DeploymentSpec,
) {
    section(lines, "Pod Template", true);
    multiline_labels(
        lines,
        "  Labels",
        spec.template
            .metadata
            .as_ref()
            .and_then(|m| m.labels.as_ref()),
    );

    let pod_spec = match &spec.template.spec {
        Some(ps) => ps,
        None => return,
    };

    field(
        lines,
        "  ServiceAccount",
        pod_spec.service_account_name.as_deref().unwrap_or("<none>"),
    );

    pod_spec
        .containers
        .iter()
        .for_each(|c| deployment_container_lines(lines, c));

    if let Some(vols) = &pod_spec.volumes {
        deployment_volume_lines(lines, vols);
    }

    multiline_labels(lines, "  Node-Selectors", pod_spec.node_selector.as_ref());

    let tols = pod_spec.tolerations.as_deref().unwrap_or(&[]);
    if tols.is_empty() {
        field(lines, "  Tolerations", "<none>");
    } else if let Some((first, rest)) = tols.split_first() {
        field(lines, "  Tolerations", &toleration_str(first));
        rest.iter()
            .for_each(|t| lines.push(format!("  {:<28}  {}", "", toleration_str(t))));
    }
}

fn deployment_section_conditions(
    lines: &mut Vec<String>,
    status: Option<&k8s_openapi::api::apps::v1::DeploymentStatus>,
) {
    section(lines, "Conditions", true);
    let conds = match status.and_then(|s| s.conditions.as_ref()) {
        Some(c) => c,
        None => return,
    };
    lines.push(format!("  {:<20}  {:<8}  {}", "Type", "Status", "Reason"));
    lines.push(format!("  {:<20}  {:<8}  {}", "────", "──────", "──────"));
    conds.iter().for_each(|c| {
        lines.push(format!(
            "  {:<20}  {:<8}  {}",
            c.type_,
            c.status,
            c.reason.as_deref().unwrap_or("")
        ))
    });
}

pub async fn describe_deployment(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
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

    section_identity(&mut lines, &d.metadata);

    if let Some(spec) = d.spec.as_ref() {
        deployment_section_spec(&mut lines, spec, d.status.as_ref());
        deployment_section_pod_template(&mut lines, spec);
    }
    deployment_section_conditions(&mut lines, d.status.as_ref());

    section(&mut lines, "Hints", true);
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
    bottom_section(&mut lines);
    lines
}

fn multiline_labels(lines: &mut Vec<String>, label: &str, map: Option<&BTreeMap<String, String>>) {
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

fn pod_section_status(lines: &mut Vec<String>, status: Option<&PodStatus>) {
    section(lines, "Status", true);
    field(
        lines,
        "Phase",
        status.and_then(|s| s.phase.as_deref()).unwrap_or("<none>"),
    );
    field(
        lines,
        "Pod IP",
        status.and_then(|s| s.pod_ip.as_deref()).unwrap_or("<none>"),
    );
    field(
        lines,
        "Host IP",
        status
            .and_then(|s| s.host_ip.as_deref())
            .unwrap_or("<none>"),
    );
}

fn format_pod_container_ports(c: &k8s_openapi::api::core::v1::Container) -> String {
    c.ports
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
        .unwrap_or_else(|| "<none>".into())
}

fn format_pod_container_limits(c: &k8s_openapi::api::core::v1::Container) -> String {
    c.resources
        .as_ref()
        .and_then(|r| r.limits.as_ref())
        .map(|l| {
            l.iter()
                .map(|(k, v)| format!("{}={}", k, v.0))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "<none>".into())
}

fn pod_section_containers(lines: &mut Vec<String>, spec: Option<&PodSpec>) {
    section(lines, "Containers", true);
    spec.map(|s| {
        s.containers.iter().for_each(|c| {
            lines.push(format!("  Container: {}", c.name));
            field(lines, "    Image", c.image.as_deref().unwrap_or("<none>"));
            field(lines, "    Ports", &format_pod_container_ports(c));
            field(lines, "    Limits", &format_pod_container_limits(c));
        })
    });
}

fn format_container_state(state: &ContainerState) -> String {
    if let Some(r) = &state.running {
        return format!(
            "Running since {}",
            r.started_at
                .as_ref()
                .map(|t| t.0.to_rfc2822())
                .unwrap_or_else(|| "?".into())
        );
    }
    if let Some(w) = &state.waiting {
        return format!("Waiting — {}", w.reason.as_deref().unwrap_or("?"));
    }
    if let Some(t) = &state.terminated {
        return format!(
            "Terminated — exit {} ({})",
            t.exit_code,
            t.reason.as_deref().unwrap_or("?")
        );
    }
    "<unknown>".into()
}

fn pod_container_status_lines(lines: &mut Vec<String>, cs: &ContainerStatus) {
    lines.push(format!("  Container: {}", cs.name));
    field(lines, "    Ready", &cs.ready.to_string());
    field(lines, "    Restart count", &cs.restart_count.to_string());
    field(lines, "    Image", &cs.image);
    cs.state
        .as_ref()
        .map(|st| field(lines, "    State", &format_container_state(st)));
}

fn pod_section_container_statuses(lines: &mut Vec<String>, status: Option<&PodStatus>) {
    section(lines, "Container Statuses", true);
    status
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs_list| {
            cs_list
                .iter()
                .for_each(|cs| pod_container_status_lines(lines, cs))
        });
}

fn pod_section_conditions(lines: &mut Vec<String>, status: Option<&PodStatus>) {
    section(lines, "Conditions", true);
    let conds = match status.and_then(|s| s.conditions.as_ref()) {
        Some(c) => c,
        None => return,
    };
    lines.push(format!("  {:<24}  {}", "Type", "Status"));
    lines.push(format!("  {:<24}  {}", "────", "──────"));
    conds
        .iter()
        .for_each(|c| lines.push(format!("  {:<24}  {}", c.type_, c.status)));
}

pub async fn describe_pod(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
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

    section_identity(&mut lines, &p.metadata);

    field(
        &mut lines,
        "Node",
        p.spec
            .as_ref()
            .and_then(|s| s.node_name.as_deref())
            .unwrap_or("<none>"),
    );
    field(
        &mut lines,
        "ServiceAccount",
        p.spec
            .as_ref()
            .and_then(|s| s.service_account_name.as_deref())
            .unwrap_or("<none>"),
    );
    pod_section_status(&mut lines, p.status.as_ref());
    pod_section_containers(&mut lines, p.spec.as_ref());
    pod_section_container_statuses(&mut lines, p.status.as_ref());
    pod_section_conditions(&mut lines, p.status.as_ref());

    section(&mut lines, "Hints", true);
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
    bottom_section(&mut lines);
    lines
}

fn section_identity(lines: &mut Vec<String>, meta: &ObjectMeta) {
    section(lines, "Identity", true);
    field(lines, "Name", opt_str(&meta.name));
    field(lines, "Namespace", opt_str(&meta.namespace));
    field(
        lines,
        "Created",
        &meta
            .creation_timestamp
            .as_ref()
            .map(|t| t.0.to_rfc2822())
            .unwrap_or_else(|| "<none>".into()),
    );
    multiline_labels(lines, "Labels", meta.labels.as_ref());
    annotations_lines(lines, "Annotations", meta.annotations.as_ref());

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
    field(lines, "Owned by", &owner);
}

fn service_section_spec(lines: &mut Vec<String>, svc: &Service) {
    section(lines, "Spec", true);

    let Some(spec) = &svc.spec else {
        field(lines, "Spec", "<none>");
        return;
    };

    field(lines, "Type", spec.type_.as_deref().unwrap_or("<none>"));
    field(
        lines,
        "ClusterIP",
        spec.cluster_ip.as_deref().unwrap_or("<none>"),
    );
    multiline_labels(lines, "Selector", spec.selector.as_ref());

    service_section_ports(lines, spec);
    service_section_network(lines, spec, &svc.status);
}

fn bottom_section(lines: &mut Vec<String>) {
    lines.push(String::new());
    lines.push(String::new());
    section(lines, "Navigational Tips", false);
    lines.push("  Esc / q — close   ↑ ↓ PgDn PgUp Home End — navigate".into());
}

fn service_section_ports(lines: &mut Vec<String>, spec: &k8s_openapi::api::core::v1::ServiceSpec) {
    let Some(ports) = &spec.ports else {
        field(lines, "Ports", "<none>");
        return;
    };

    if ports.is_empty() {
        field(lines, "Ports", "<none>");
        return;
    }

    for (i, p) in ports.iter().enumerate() {
        let proto = p.protocol.as_deref().unwrap_or("TCP");
        let name = p.name.as_deref().unwrap_or("");

        let target = p
            .target_port
            .as_ref()
            .map(int_or_str)
            .unwrap_or_else(|| p.port.to_string());

        let node = p
            .node_port
            .map(|n| format!(" NodePort={}", n))
            .unwrap_or_default();

        let line = format!("{}/{} {} -> {}{}", p.port, proto, name, target, node);

        if i == 0 {
            field(lines, "Ports", &line);
        } else {
            lines.push(format!("  {:<28}  {}", "", line));
        }
    }
}

fn service_section_network(
    lines: &mut Vec<String>,
    spec: &k8s_openapi::api::core::v1::ServiceSpec,
    status: &Option<k8s_openapi::api::core::v1::ServiceStatus>,
) {
    let external_ips = spec
        .external_ips
        .as_ref()
        .map(|v| v.join(", "))
        .unwrap_or_else(|| "<none>".into());

    field(lines, "External IPs", &external_ips);

    if let Some(s) = status {
        if let Some(lb) = &s.load_balancer {
            if let Some(ing) = &lb.ingress {
                let ips: Vec<_> = ing
                    .iter()
                    .filter_map(|i| i.ip.clone().or_else(|| i.hostname.clone()))
                    .collect();

                if !ips.is_empty() {
                    field(lines, "LoadBalancer", &ips.join(", "));
                }
            }
        }
    }

    if let Some(f) = &spec.ip_families {
        field(lines, "IP Families", &f.join(", "));
    }

    if let Some(p) = &spec.ip_family_policy {
        field(lines, "IP Policy", p);
    }
}

pub async fn describe_service(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
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

    section_identity(&mut lines, &svc.metadata);
    service_section_spec(&mut lines, &svc);

    section(&mut lines, "Hints", true);
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

    bottom_section(&mut lines);

    lines
}

pub async fn describe_replicaset(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
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

    section_identity(&mut lines, &meta);

    section(&mut lines, "Replicas", true);
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

    section(&mut lines, "Pod Template", true);
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

    section(&mut lines, "Hints", true);
    lines.push(format!(
        "  kubectl describe replicaset/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    bottom_section(&mut lines);
    lines
}

pub async fn describe_daemonset(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
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

    section_identity(&mut lines, &meta);

    section(&mut lines, "Status", true);
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

    section(&mut lines, "Pod Template", true);
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

    section(&mut lines, "Hints", true);
    lines.push(format!(
        "  kubectl describe daemonset/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    bottom_section(&mut lines);
    lines
}

pub async fn describe_job(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
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

    section_identity(&mut lines, &meta);

    section(&mut lines, "Spec", true);
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

    section(&mut lines, "Status", true);
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

    section(&mut lines, "Conditions", true);
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

    section(&mut lines, "Hints", true);
    lines.push(format!(
        "  kubectl describe job/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));
    bottom_section(&mut lines);
    lines
}

pub async fn describe_pv(client: &Client, name: &str) -> Vec<String> {
    let api: Api<PersistentVolume> = Api::all(client.clone());
    let mut lines = Vec::new();

    let pv = match api.get(name).await {
        Ok(pv) => pv,
        Err(e) => {
            lines.push(format!(
                "  Error fetching PersistentVolume '{}': {}",
                name, e
            ));
            return lines;
        }
    };

    let meta = &pv.metadata;
    let spec = pv.spec.as_ref();
    let status = pv.status.as_ref();

    section_identity(&mut lines, meta);

    section(&mut lines, "Spec", true);

    field(
        &mut lines,
        "Capacity",
        &spec
            .and_then(|s| s.capacity.as_ref())
            .and_then(|c| c.get("storage"))
            .map(|q| q.0.clone())
            .unwrap_or_else(|| "<none>".into()),
    );

    field(
        &mut lines,
        "Access Modes",
        &spec
            .and_then(|s| s.access_modes.as_ref())
            .map(|m| m.join(", "))
            .unwrap_or_else(|| "<none>".into()),
    );

    field(
        &mut lines,
        "Reclaim Policy",
        spec.and_then(|s| s.persistent_volume_reclaim_policy.as_deref())
            .unwrap_or("<none>"),
    );

    field(
        &mut lines,
        "Storage Class",
        spec.and_then(|s| s.storage_class_name.as_deref())
            .unwrap_or("<none>"),
    );

    field(
        &mut lines,
        "Volume Mode",
        spec.and_then(|s| s.volume_mode.as_deref())
            .unwrap_or("<none>"),
    );

    if let Some(claim) = spec.and_then(|s| s.claim_ref.as_ref()) {
        field(
            &mut lines,
            "Claim",
            &format!(
                "{}/{}",
                claim.namespace.as_deref().unwrap_or("?"),
                claim.name.as_deref().unwrap_or("?")
            ),
        );
    }

    if let Some(s) = spec {
        if let Some(host) = &s.host_path {
            field(&mut lines, "Source", &format!("HostPath: {}", host.path));
        } else if let Some(nfs) = &s.nfs {
            field(
                &mut lines,
                "Source",
                &format!("NFS: {}:{}", nfs.server, nfs.path),
            );
        } else if let Some(aws) = &s.aws_elastic_block_store {
            field(&mut lines, "Source", &format!("AWS EBS: {}", aws.volume_id));
        } else if let Some(csi) = &s.csi {
            field(
                &mut lines,
                "Source",
                &format!("CSI: {} ({})", csi.driver, csi.volume_handle),
            );
        } else {
            field(&mut lines, "Source", "<unknown>");
        }
    }

    section(&mut lines, "Status", true);

    field(
        &mut lines,
        "Phase",
        status.and_then(|s| s.phase.as_deref()).unwrap_or("<none>"),
    );

    section(&mut lines, "Status", true);

    field(
        &mut lines,
        "Phase",
        status.and_then(|s| s.phase.as_deref()).unwrap_or("<none>"),
    );

    field(
        &mut lines,
        "Reason",
        status.and_then(|s| s.reason.as_deref()).unwrap_or("<none>"),
    );

    field(
        &mut lines,
        "Message",
        status
            .and_then(|s| s.message.as_deref())
            .unwrap_or("<none>"),
    );

    field(
        &mut lines,
        "Last Transition",
        &status
            .and_then(|s| s.last_phase_transition_time.as_ref())
            .map(|t| t.0.to_rfc2822())
            .unwrap_or_else(|| "<none>".into()),
    );

    section(&mut lines, "Hints", true);

    lines.push(format!("  kubectl describe pv/{}", name));
    lines.push(format!("  kubectl get pv/{} -o yaml", name));

    bottom_section(&mut lines);

    lines
}

pub async fn describe_pvc(client: &Client, name: &str, ns: Option<&str>) -> Vec<String> {
    let api: Api<PersistentVolumeClaim> = match ns {
        Some(n) => Api::namespaced(client.clone(), n),
        None => Api::all(client.clone()),
    };

    let mut lines = Vec::new();

    let pvc = match api.get(name).await {
        Ok(pvc) => pvc,
        Err(e) => {
            lines.push(format!("  Error fetching PVC '{}': {}", name, e));
            return lines;
        }
    };

    let meta = &pvc.metadata;
    let spec = pvc.spec.as_ref();
    let status = pvc.status.as_ref();

    section_identity(&mut lines, meta);

    section(&mut lines, "Spec", true);

    field(
        &mut lines,
        "Volume",
        &spec
            .and_then(|s| s.volume_name.clone())
            .unwrap_or_else(|| "<none>".into()),
    );

    field(
        &mut lines,
        "StorageClass",
        &spec
            .and_then(|s| s.storage_class_name.clone())
            .unwrap_or_else(|| "<none>".into()),
    );

    field(
        &mut lines,
        "AccessModes",
        &spec
            .and_then(|s| s.access_modes.clone())
            .map(|m| m.join(", "))
            .unwrap_or_else(|| "<none>".into()),
    );

    field(
        &mut lines,
        "Requested Storage",
        &spec
            .and_then(|s| s.resources.as_ref())
            .and_then(|r| r.requests.as_ref())
            .and_then(|req| req.get("storage"))
            .map(|q| q.0.to_string())
            .unwrap_or_else(|| "<none>".into()),
    );

    section(&mut lines, "Status", true);

    field(
        &mut lines,
        "Phase",
        &status
            .and_then(|s| s.phase.clone())
            .unwrap_or_else(|| "<none>".into()),
    );

    field(
        &mut lines,
        "Capacity",
        &status
            .and_then(|s| s.capacity.as_ref())
            .and_then(|c| c.get("storage"))
            .map(|q| q.0.to_string())
            .unwrap_or_else(|| "<none>".into()),
    );

    field(
        &mut lines,
        "AccessModes",
        &status
            .and_then(|s| s.access_modes.clone())
            .map(|m| m.join(", "))
            .unwrap_or_else(|| "<none>".into()),
    );

    field(
        &mut lines,
        "Conditions",
        &status
            .and_then(|s| s.conditions.as_ref())
            .map(|conds| {
                conds
                    .iter()
                    .map(|c| format!("{}={}", c.type_, c.status))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| "<none>".into()),
    );

    section(&mut lines, "Hints", true);

    lines.push(format!(
        "  kubectl describe pvc/{} -n {}",
        name,
        ns.unwrap_or("default")
    ));

    bottom_section(&mut lines);

    lines
}
