use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, sqlx::FromRow)]
pub struct HostNumaNode {
    pub id: Uuid,
    pub host_id: Uuid,
    pub node_id: i32,
    pub cpu_list: String,
    pub memory_bytes: Option<i64>,
    #[schema(value_type = Vec<i32>)]
    pub distances: Vec<i32>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// NUMA node info from node discovery (mirrors proto NumaNode)
pub struct NumaNodeDiscovery {
    pub node_id: i32,
    pub cpu_list: String,
    pub memory_bytes: Option<i64>,
    pub distances: Vec<i32>,
}

/// List all NUMA nodes for a host.
pub async fn list_by_host(pool: &PgPool, host_id: Uuid) -> Result<Vec<HostNumaNode>, sqlx::Error> {
    sqlx::query_as::<_, HostNumaNode>(
        r#"
SELECT id, host_id, node_id, cpu_list, memory_bytes, distances, updated_at
FROM host_numa_nodes
WHERE host_id = $1
ORDER BY node_id
        "#,
    )
    .bind(host_id)
    .fetch_all(pool)
    .await
}

/// Upsert discovered NUMA nodes, removing stale entries for this host.
pub async fn sync_numa_nodes(
    pool: &PgPool,
    host_id: Uuid,
    nodes: &[NumaNodeDiscovery],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    for node in nodes {
        sqlx::query(
            r#"
INSERT INTO host_numa_nodes (host_id, node_id, cpu_list, memory_bytes, distances, updated_at)
VALUES ($1, $2, $3, $4, $5, NOW())
ON CONFLICT (host_id, node_id)
DO UPDATE SET cpu_list     = EXCLUDED.cpu_list,
              memory_bytes = EXCLUDED.memory_bytes,
              distances    = EXCLUDED.distances,
              updated_at   = NOW()
            "#,
        )
        .bind(host_id)
        .bind(node.node_id)
        .bind(&node.cpu_list)
        .bind(node.memory_bytes)
        .bind(&node.distances)
        .execute(tx.as_mut())
        .await?;
    }

    // Remove nodes that are no longer reported by the host
    if nodes.is_empty() {
        sqlx::query("DELETE FROM host_numa_nodes WHERE host_id = $1")
            .bind(host_id)
            .execute(tx.as_mut())
            .await?;
    } else {
        let node_ids: Vec<i32> = nodes.iter().map(|n| n.node_id).collect();
        sqlx::query("DELETE FROM host_numa_nodes WHERE host_id = $1 AND node_id != ALL($2)")
            .bind(host_id)
            .bind(&node_ids)
            .execute(tx.as_mut())
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Convert a Vec of CPU IDs back into a compact cpu_list string (e.g. "0-3,8").
pub fn expand_cpu_list_to_string(cpus: &[i32]) -> String {
    if cpus.is_empty() {
        return String::new();
    }
    let mut sorted = cpus.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let mut ranges: Vec<String> = Vec::new();
    let mut start = sorted[0];
    let mut end = sorted[0];

    for &cpu in &sorted[1..] {
        if cpu == end + 1 {
            end = cpu;
        } else {
            if start == end {
                ranges.push(start.to_string());
            } else {
                ranges.push(format!("{}-{}", start, end));
            }
            start = cpu;
            end = cpu;
        }
    }
    if start == end {
        ranges.push(start.to_string());
    } else {
        ranges.push(format!("{}-{}", start, end));
    }
    ranges.join(",")
}

#[cfg(test)]
mod tests {
    use super::expand_cpu_list_to_string;
    use common::cpu_list::expand_cpu_list;

    #[test]
    fn expand_cpu_list_range() {
        assert_eq!(expand_cpu_list("0-3"), vec![0, 1, 2, 3]);
    }

    #[test]
    fn expand_cpu_list_single() {
        assert_eq!(expand_cpu_list("8"), vec![8]);
    }

    #[test]
    fn expand_cpu_list_mixed() {
        assert_eq!(expand_cpu_list("0-3,8"), vec![0, 1, 2, 3, 8]);
    }

    #[test]
    fn expand_cpu_list_empty() {
        assert_eq!(expand_cpu_list(""), Vec::<i32>::new());
    }

    #[test]
    fn expand_cpu_list_sorted() {
        // Out-of-order input should still produce sorted output
        assert_eq!(expand_cpu_list("8,0-3"), vec![0, 1, 2, 3, 8]);
    }

    #[test]
    fn expand_cpu_list_to_string_range() {
        assert_eq!(expand_cpu_list_to_string(&[0, 1, 2, 3]), "0-3");
    }

    #[test]
    fn expand_cpu_list_to_string_single() {
        assert_eq!(expand_cpu_list_to_string(&[8]), "8");
    }

    #[test]
    fn expand_cpu_list_to_string_mixed() {
        assert_eq!(expand_cpu_list_to_string(&[0, 1, 2, 3, 8]), "0-3,8");
    }

    #[test]
    fn expand_cpu_list_to_string_empty() {
        assert_eq!(expand_cpu_list_to_string(&[]), "");
    }

    #[test]
    fn expand_cpu_list_to_string_dedup() {
        assert_eq!(expand_cpu_list_to_string(&[0, 0, 1, 2]), "0-2");
    }

    #[test]
    fn roundtrip() {
        let cpus = vec![0, 1, 2, 3, 8, 10, 11];
        let s = expand_cpu_list_to_string(&cpus);
        assert_eq!(expand_cpu_list(&s), cpus);
    }
}
