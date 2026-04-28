use uuid::Uuid;

use crate::client::Client;

use super::models::{
    AttachSecurityGroupRequest, NewSecurityGroup, NewSecurityGroupRule, SecurityGroup,
    SecurityGroupRule,
};

pub async fn list(client: &Client, name: Option<&str>) -> anyhow::Result<Vec<SecurityGroup>> {
    let path = match name {
        Some(name) => format!("/security-groups?name={}", urlencoding::encode(name)),
        None => "/security-groups".to_string(),
    };
    client.get(&path).await
}

pub async fn get(client: &Client, security_group_id: Uuid) -> anyhow::Result<SecurityGroup> {
    client
        .get(&format!("/security-groups/{security_group_id}"))
        .await
}

pub async fn create(client: &Client, group: &NewSecurityGroup) -> anyhow::Result<String> {
    client.post_text("/security-groups", group).await
}

pub async fn delete(client: &Client, security_group_id: Uuid) -> anyhow::Result<()> {
    client
        .delete(&format!("/security-groups/{security_group_id}"))
        .await
}

pub async fn list_rules(
    client: &Client,
    security_group_id: Uuid,
) -> anyhow::Result<Vec<SecurityGroupRule>> {
    client
        .get(&format!("/security-groups/{security_group_id}/rules"))
        .await
}

pub async fn create_rule(
    client: &Client,
    security_group_id: Uuid,
    rule: &NewSecurityGroupRule,
) -> anyhow::Result<String> {
    client
        .post_text(&format!("/security-groups/{security_group_id}/rules"), rule)
        .await
}

pub async fn delete_rule(
    client: &Client,
    security_group_id: Uuid,
    rule_id: Uuid,
) -> anyhow::Result<()> {
    client
        .delete(&format!(
            "/security-groups/{security_group_id}/rules/{rule_id}"
        ))
        .await
}

pub async fn attach_to_vm(
    client: &Client,
    vm_id: Uuid,
    security_group_id: Uuid,
) -> anyhow::Result<()> {
    client
        .post_response(
            &format!("/vms/{vm_id}/security-groups"),
            &AttachSecurityGroupRequest { security_group_id },
        )
        .await?;
    Ok(())
}

pub async fn detach_from_vm(
    client: &Client,
    vm_id: Uuid,
    security_group_id: Uuid,
) -> anyhow::Result<()> {
    client
        .delete(&format!("/vms/{vm_id}/security-groups/{security_group_id}"))
        .await
}
