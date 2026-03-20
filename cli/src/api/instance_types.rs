use uuid::Uuid;

use crate::client::Client;

use super::models::{InstanceType, NewInstanceType};

pub async fn list(client: &Client, name: Option<&str>) -> anyhow::Result<Vec<InstanceType>> {
    let path = match name {
        Some(n) => format!("/instance-types?name={n}"),
        None => "/instance-types".to_string(),
    };
    client.get(&path).await
}

pub async fn get(client: &Client, id: Uuid) -> anyhow::Result<InstanceType> {
    client.get(&format!("/instance-types/{id}")).await
}

pub async fn create(client: &Client, instance_type: &NewInstanceType) -> anyhow::Result<String> {
    client.post_text("/instance-types", instance_type).await
}

pub async fn delete(client: &Client, id: Uuid) -> anyhow::Result<()> {
    client.delete(&format!("/instance-types/{id}")).await
}
