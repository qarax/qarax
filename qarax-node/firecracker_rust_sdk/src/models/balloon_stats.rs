/*
 * Firecracker API
 *
 * RESTful public-facing API. The API is accessible through HTTP calls on specific URLs carrying JSON modeled data. The transport medium is a Unix Domain Socket.
 *
 * The version of the OpenAPI document: 1.0.0
 * Contact: compute-capsule@amazon.com
 * Generated by: https://openapi-generator.tech
 */

/// BalloonStats : Describes the balloon device statistics.

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct BalloonStats {
    /// Target number of pages the device aims to hold.
    #[serde(rename = "target_pages")]
    pub target_pages: i32,
    /// Actual number of pages the device is holding.
    #[serde(rename = "actual_pages")]
    pub actual_pages: i32,
    /// Target amount of memory (in MiB) the device aims to hold.
    #[serde(rename = "target_mib")]
    pub target_mib: i32,
    /// Actual amount of memory (in MiB) the device is holding.
    #[serde(rename = "actual_mib")]
    pub actual_mib: i32,
    /// The amount of memory that has been swapped in (in bytes).
    #[serde(rename = "swap_in", skip_serializing_if = "Option::is_none")]
    pub swap_in: Option<i64>,
    /// The amount of memory that has been swapped out to disk (in bytes).
    #[serde(rename = "swap_out", skip_serializing_if = "Option::is_none")]
    pub swap_out: Option<i64>,
    /// The number of major page faults that have occurred.
    #[serde(rename = "major_faults", skip_serializing_if = "Option::is_none")]
    pub major_faults: Option<i64>,
    /// The number of minor page faults that have occurred.
    #[serde(rename = "minor_faults", skip_serializing_if = "Option::is_none")]
    pub minor_faults: Option<i64>,
    /// The amount of memory not being used for any purpose (in bytes).
    #[serde(rename = "free_memory", skip_serializing_if = "Option::is_none")]
    pub free_memory: Option<i64>,
    /// The total amount of memory available (in bytes).
    #[serde(rename = "total_memory", skip_serializing_if = "Option::is_none")]
    pub total_memory: Option<i64>,
    /// An estimate of how much memory is available (in bytes) for starting new applications, without pushing the system to swap.
    #[serde(rename = "available_memory", skip_serializing_if = "Option::is_none")]
    pub available_memory: Option<i64>,
    /// The amount of memory, in bytes, that can be quickly reclaimed without additional I/O. Typically these pages are used for caching files from disk.
    #[serde(rename = "disk_caches", skip_serializing_if = "Option::is_none")]
    pub disk_caches: Option<i64>,
    /// The number of successful hugetlb page allocations in the guest.
    #[serde(
        rename = "hugetlb_allocations",
        skip_serializing_if = "Option::is_none"
    )]
    pub hugetlb_allocations: Option<i64>,
    /// The number of failed hugetlb page allocations in the guest.
    #[serde(rename = "hugetlb_failures", skip_serializing_if = "Option::is_none")]
    pub hugetlb_failures: Option<i64>,
}

impl BalloonStats {
    /// Describes the balloon device statistics.
    pub fn new(
        target_pages: i32,
        actual_pages: i32,
        target_mib: i32,
        actual_mib: i32,
    ) -> BalloonStats {
        BalloonStats {
            target_pages,
            actual_pages,
            target_mib,
            actual_mib,
            swap_in: None,
            swap_out: None,
            major_faults: None,
            minor_faults: None,
            free_memory: None,
            total_memory: None,
            available_memory: None,
            disk_caches: None,
            hugetlb_allocations: None,
            hugetlb_failures: None,
        }
    }
}
