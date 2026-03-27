use opentelemetry::{KeyValue, global};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_otlp::{MetricExporter, SpanExporter};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};

/// Configuration for OpenTelemetry providers.
pub struct OtelConfig {
    pub service_name: String,
    pub service_version: String,
    pub otlp_endpoint: String,
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            service_name: "qarax".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            otlp_endpoint: "http://localhost:4318".to_string(),
        }
    }
}

/// Holds initialized OTel providers for graceful shutdown.
pub struct OtelGuard {
    pub tracer_provider: SdkTracerProvider,
    pub meter_provider: SdkMeterProvider,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Err(e) = self.tracer_provider.shutdown() {
            eprintln!("Failed to shutdown tracer provider: {e}");
        }
        if let Err(e) = self.meter_provider.shutdown() {
            eprintln!("Failed to shutdown meter provider: {e}");
        }
    }
}

fn signal_endpoint(base: &str, signal: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    let prefix = trimmed
        .strip_suffix("/v1/traces")
        .or_else(|| trimmed.strip_suffix("/v1/metrics"))
        .unwrap_or(trimmed);

    format!("{prefix}/v1/{signal}")
}

/// Initialize OpenTelemetry tracer and meter providers with OTLP/HTTP export.
///
/// Returns an `OtelGuard` that must be kept alive for the lifetime of the
/// application. Dropping it flushes and shuts down the providers.
pub fn init_providers(config: OtelConfig) -> Result<OtelGuard, Box<dyn std::error::Error>> {
    // Set the W3C TraceContext propagator for distributed tracing
    global::set_text_map_propagator(TraceContextPropagator::new());

    let resource = Resource::builder()
        .with_attributes([
            KeyValue::new(SERVICE_NAME, config.service_name.clone()),
            KeyValue::new(SERVICE_VERSION, config.service_version),
        ])
        .build();

    let span_exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(signal_endpoint(&config.otlp_endpoint, "traces"))
        .build()?;

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(resource.clone())
        .build();

    global::set_tracer_provider(tracer_provider.clone());

    let metric_exporter = MetricExporter::builder()
        .with_http()
        .with_endpoint(signal_endpoint(&config.otlp_endpoint, "metrics"))
        .build()?;

    let metric_reader = PeriodicReader::builder(metric_exporter).build();

    let meter_provider = SdkMeterProvider::builder()
        .with_reader(metric_reader)
        .with_resource(resource)
        .build();

    global::set_meter_provider(meter_provider.clone());

    Ok(OtelGuard {
        tracer_provider,
        meter_provider,
    })
}

#[cfg(test)]
mod tests {
    use super::signal_endpoint;

    #[test]
    fn appends_trace_path_to_base_endpoint() {
        assert_eq!(
            signal_endpoint("http://localhost:4318", "traces"),
            "http://localhost:4318/v1/traces"
        );
    }

    #[test]
    fn appends_metric_path_to_base_endpoint_with_custom_prefix() {
        assert_eq!(
            signal_endpoint("http://collector:4318/otel", "metrics"),
            "http://collector:4318/otel/v1/metrics"
        );
    }

    #[test]
    fn rewrites_existing_signal_path_for_other_signal() {
        assert_eq!(
            signal_endpoint("http://localhost:4318/v1/traces", "metrics"),
            "http://localhost:4318/v1/metrics"
        );
    }
}
