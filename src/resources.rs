use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet};
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{Namespace, Pod, Service};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::{ClusterResourceScope, NamespaceResourceScope};
use kube::{api::ListParams, Api, Client, Resource};
use serde::de::DeserializeOwned;
use std::fmt::Debug;

pub struct ResourceRow {
    pub name: String,
    pub data: Vec<String>,
}

pub trait IntoResourceRow {
    fn into_resource_row(self) -> ResourceRow;
}

pub async fn fetch_resources<K>(client: &Client, ns: &Option<String>) -> Vec<ResourceRow>
where
    K: Resource<Scope = NamespaceResourceScope>
        + Clone
        + Debug
        + DeserializeOwned
        + IntoResourceRow
        + 'static,
    K::DynamicType: Default,
{
    let api: Api<K> = ns.as_ref().map_or(Api::all(client.clone()), |n| {
        Api::namespaced(client.clone(), n)
    });

    api.list(&ListParams::default())
        .await
        .map(|l| {
            l.items
                .into_iter()
                .map(|item| item.into_resource_row())
                .collect()
        })
        .unwrap_or_default()
}

pub async fn fetch_cluster_resources<K>(client: &Client) -> Vec<ResourceRow>
where
    K: Resource<Scope = ClusterResourceScope>
        + Clone
        + Debug
        + DeserializeOwned
        + IntoResourceRow
        + 'static,
    K::DynamicType: Default,
{
    Api::<K>::all(client.clone())
        .list(&ListParams::default())
        .await
        .map(|l| {
            l.items
                .into_iter()
                .map(|item| item.into_resource_row())
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

impl IntoResourceRow for Namespace {
    fn into_resource_row(self) -> ResourceRow {
        let age = get_age(&self.metadata);
        let name = self.metadata.name.unwrap_or_default();

        ResourceRow {
            name,
            data: vec![age],
        }
    }
}

impl IntoResourceRow for Pod {
    fn into_resource_row(self) -> ResourceRow {
        let status = self.status.as_ref();

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

        let age = get_age(&self.metadata);
        let name = self.metadata.name.unwrap_or_default();

        ResourceRow {
            name: name,
            data: vec![ready, phase, restarts, age],
        }
    }
}

impl IntoResourceRow for Service {
    fn into_resource_row(self) -> ResourceRow {
        let spec = self.spec.as_ref();

        let type_ = spec.and_then(|sp| sp.type_.clone()).unwrap_or_default();

        let cluster_ip = spec
            .and_then(|sp| sp.cluster_ip.clone())
            .unwrap_or_else(|| "<none>".into());

        let external_ip = self
            .status
            .as_ref()
            .and_then(|st| st.load_balancer.as_ref())
            .and_then(|lb| lb.ingress.as_ref())
            .and_then(|ing| ing.first())
            .and_then(|i| i.ip.clone().or_else(|| i.hostname.clone()))
            .unwrap_or_else(|| "<none>".into());

        let ports = spec
            .and_then(|sp| sp.ports.as_ref())
            .map(|ps| {
                ps.iter()
                    .map(|p| format!("{}/{}", p.port, p.protocol.as_deref().unwrap_or("TCP")))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();

        let age = get_age(&self.metadata);
        let name = self.metadata.name.unwrap_or_default();

        ResourceRow {
            name: name,
            data: vec![type_, cluster_ip, external_ip, ports, age],
        }
    }
}

impl IntoResourceRow for Deployment {
    fn into_resource_row(self) -> ResourceRow {
        let status = self.status.as_ref();

        let ready = format!(
            "{}/{}",
            status.and_then(|s| s.ready_replicas).unwrap_or(0),
            status.and_then(|s| s.replicas).unwrap_or(0),
        );

        let up_to_date = status
            .and_then(|s| s.updated_replicas)
            .unwrap_or(0)
            .to_string();

        let available = status
            .and_then(|s| s.available_replicas)
            .unwrap_or(0)
            .to_string();

        let age = get_age(&self.metadata);
        let name = self.metadata.name.unwrap_or_default();

        ResourceRow {
            name: name,
            data: vec![ready, up_to_date, available, age],
        }
    }
}

impl IntoResourceRow for ReplicaSet {
    fn into_resource_row(self) -> ResourceRow {
        let status = self.status.as_ref();

        let desired = self
            .spec
            .as_ref()
            .and_then(|sp| sp.replicas)
            .unwrap_or(0)
            .to_string();

        let current = status.map(|s| s.replicas).unwrap_or(0).to_string();

        let ready = status
            .and_then(|s| s.ready_replicas)
            .unwrap_or(0)
            .to_string();

        let age = get_age(&self.metadata);
        let name = self.metadata.name.unwrap_or_default();

        ResourceRow {
            name: name,
            data: vec![desired, current, ready, age],
        }
    }
}

impl IntoResourceRow for DaemonSet {
    fn into_resource_row(self) -> ResourceRow {
        let status = self.status.as_ref();

        let desired = status
            .map(|s| s.desired_number_scheduled)
            .unwrap_or(0)
            .to_string();

        let current = status
            .map(|s| s.current_number_scheduled)
            .unwrap_or(0)
            .to_string();

        let ready = status.map(|s| s.number_ready).unwrap_or(0).to_string();

        let age = get_age(&self.metadata);
        let name = self.metadata.name.unwrap_or_default();

        ResourceRow {
            name: name,
            data: vec![desired, current, ready, age],
        }
    }
}

impl IntoResourceRow for Job {
    fn into_resource_row(self) -> ResourceRow {
        let status = self.status.as_ref();

        let completions = format!(
            "{}/{}",
            status.and_then(|s| s.succeeded).unwrap_or(0),
            self.spec
                .as_ref()
                .and_then(|sp| sp.completions)
                .unwrap_or(1),
        );

        let active = status.and_then(|s| s.active).unwrap_or(0).to_string();

        let failed = status.and_then(|s| s.failed).unwrap_or(0).to_string();

        let age = get_age(&self.metadata);
        let name = self.metadata.name.unwrap_or_default();

        ResourceRow {
            name: name,
            data: vec![completions, active, failed, age],
        }
    }
}
