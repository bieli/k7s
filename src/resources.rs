use kube::{Api, Client, Resource, api::ListParams};
use k8s_openapi::NamespaceResourceScope;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use k8s_openapi::api::core::v1::{Pod};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;


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
    let api: Api<K> = ns
        .as_ref()
        .map_or(Api::all(client.clone()), |n| Api::namespaced(client.clone(), n));

    api.list(&ListParams::default())
        .await
        .map(|l| l.items.into_iter().map(|item| item.into_resource_row()).collect())
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

impl IntoResourceRow for Pod {
    fn into_resource_row(self) -> ResourceRow {
        let status = self.status.as_ref();

        let ready = status
            .and_then(|s| s.container_statuses.as_ref())
            .map(|cs| {
                format!(
                    "{}/{}",
                    cs.iter().filter(|c| c.ready).count(),
                    cs.len()
                )
            })
            .unwrap_or_else(|| "0/0".into());

        let phase = status
            .and_then(|s| s.phase.clone())
            .unwrap_or_else(|| "Unknown".into());

        let restarts = status
            .and_then(|s| s.container_statuses.as_ref())
            .map(|cs| {
                cs.iter()
                    .map(|c| c.restart_count)
                    .sum::<i32>()
                    .to_string()
            })
            .unwrap_or_else(|| "0".into());

        let age  = get_age(&self.metadata);
        let name = self.metadata.name.unwrap_or_default();

        ResourceRow {
            name: name,
            data: vec![ready, phase, restarts, age],
        }
    }
}

