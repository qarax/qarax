use uuid::Uuid;

use crate::client::Client;

use super::models::{InstanceType, NewInstanceType};

pub async fn list(client: &Client) -> anyhow::Result<Vec<InstanceType>> {
    client.get("/instance-types").await
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
