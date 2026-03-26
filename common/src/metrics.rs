use opentelemetry::metrics::{Counter, Histogram, Meter};

/// Application-level metrics for qarax.
///
/// Create once during startup and share via `Arc<Metrics>`.
pub struct Metrics {
    /// Total VM operations dispatched to nodes (labels: operation, status)
    pub vm_operations_total: Counter<u64>,
    /// Duration of VM operations in seconds (labels: operation)
    pub vm_operation_duration_seconds: Histogram<f64>,
    /// Duration of gRPC client calls in seconds (labels: method)
    pub grpc_client_duration_seconds: Histogram<f64>,
    /// Total gRPC client errors (labels: method)
    pub grpc_client_errors_total: Counter<u64>,
    /// Duration of background monitor cycles in seconds (labels: monitor)
    pub monitor_cycle_duration_seconds: Histogram<f64>,
    /// Total background monitor cycles (labels: monitor)
    pub monitor_cycles_total: Counter<u64>,
}

impl Metrics {
    pub fn new(meter: &Meter) -> Self {
        Self {
            vm_operations_total: meter
                .u64_counter("qarax.vm.operations.total")
                .with_description("Total VM operations dispatched to nodes")
                .build(),
            vm_operation_duration_seconds: meter
                .f64_histogram("qarax.vm.operation.duration")
                .with_description("Duration of VM operations in seconds")
                .with_unit("s")
                .build(),
            grpc_client_duration_seconds: meter
                .f64_histogram("qarax.grpc.client.duration")
                .with_description("Duration of gRPC client calls in seconds")
                .with_unit("s")
                .build(),
            grpc_client_errors_total: meter
                .u64_counter("qarax.grpc.client.errors.total")
                .with_description("Total gRPC client errors")
                .build(),
            monitor_cycle_duration_seconds: meter
                .f64_histogram("qarax.monitor.cycle.duration")
                .with_description("Duration of background monitor cycles in seconds")
                .with_unit("s")
                .build(),
            monitor_cycles_total: meter
                .u64_counter("qarax.monitor.cycles.total")
                .with_description("Total background monitor cycles")
                .build(),
        }
    }
}
