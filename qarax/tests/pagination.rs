mod support;

use reqwest::StatusCode;
use serde_json::{Value, json};
use support::{add_host, add_up_host, create_vm, spawn_app};

/// Every page of `url` (which has a query string) with `limit` items each.
async fn pages(client: &reqwest::Client, url: &str, limit: u32) -> Vec<Vec<Value>> {
    let mut pages = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut page_url = format!("{url}&limit={limit}");
        if let Some(cursor) = &cursor {
            page_url.push_str(&format!("&cursor={cursor}"));
        }
        let res = client.get(page_url).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let page: Value = res.json().await.unwrap();
        pages.push(page["items"].as_array().unwrap().clone());
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => return pages,
        }
    }
}

fn field(items: &[Value], name: &str) -> Vec<String> {
    items
        .iter()
        .map(|item| item[name].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn lists_by_name_page_through_every_item_in_order() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    for name in ["charlie", "alpha", "bravo"] {
        let res = client
            .post(format!("{}/instance-types", app.address))
            .json(&json!({"name": name, "boot_vcpus": 1, "max_vcpus": 1, "memory_size": 268435456}))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    let url = format!("{}/instance-types?", app.address);
    let pages = pages(&client, &url, 2).await;
    let names: Vec<Vec<String>> = pages.iter().map(|page| field(page, "name")).collect();
    assert_eq!(names, vec![vec!["alpha", "bravo"], vec!["charlie"]]);
}

#[tokio::test]
async fn hosts_page_through_every_host_by_name() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    for name in ["host-c", "host-a", "host-b"] {
        add_host(&client, &app.address, name).await;
    }

    let url = format!("{}/hosts?", app.address);
    let pages = pages(&client, &url, 2).await;
    let names: Vec<Vec<String>> = pages.iter().map(|page| field(page, "name")).collect();
    assert_eq!(names, vec![vec!["host-a", "host-b"], vec!["host-c"]]);
}

#[tokio::test]
async fn vm_pages_hold_only_the_vms_the_filter_matches() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "host").await;
    for (name, tags) in [
        ("vm-d", vec!["batch"]),
        ("vm-a", vec!["batch"]),
        ("vm-c", vec![]),
        ("vm-b", vec!["batch"]),
    ] {
        let body = json!({
            "name": name,
            "tags": tags,
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        });
        create_vm(&client, &app.address, body).await;
    }

    let url = format!("{}/vms?tags=batch", app.address);
    let pages = pages(&client, &url, 2).await;
    let names: Vec<Vec<String>> = pages.iter().map(|page| field(page, "name")).collect();
    assert_eq!(names, vec![vec!["vm-a", "vm-b"], vec!["vm-d"]]);
}

#[tokio::test]
async fn lists_newest_first_page_through_every_item_in_order() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let mut created = Vec::new();
    for name in ["host-1", "host-2", "host-3"] {
        created.push(add_host(&client, &app.address, name).await.to_string());
    }

    let url = format!(
        "{}/audit-logs?resource_type=host&action=create",
        app.address
    );
    let pages = pages(&client, &url, 2).await;
    assert_eq!(pages.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 1]);
    let ids: Vec<String> = pages
        .concat()
        .iter()
        .map(|log| log["resource_id"].as_str().unwrap().to_string())
        .collect();
    created.reverse();
    assert_eq!(ids, created);
}

#[tokio::test]
async fn invalid_page_parameters_are_refused() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let not_a_uuid = hex::encode("not-a-uuid:alpha");
    for query in [
        "limit=0".to_string(),
        "limit=1001".to_string(),
        "cursor=zz".to_string(),
        format!("cursor={not_a_uuid}"),
    ] {
        let res = client
            .get(format!("{}/instance-types?{query}", app.address))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{query}");
    }
}
