use tracing_bunyan_formatter::BunyanFormattingLayer;
use tracing_bunyan_formatter::JsonStorageLayer;
use tracing_subscriber::{fmt::MakeWriter, prelude::__tracing_subscriber_SubscriberExt};

#[cfg(not(feature = "otel"))]
pub fn get_subscriber<Sink>(
    name: String,
    env_filter: String,
    sink: Sink,
) -> impl tracing::Subscriber + Send + Sync
where
    Sink: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(env_filter));
    let formatting_layer = BunyanFormattingLayer::new(name, sink);
    tracing_subscriber::Registry::default()
        .with(env_filter)
        .with(formatting_layer)
        .with(JsonStorageLayer)
}

#[cfg(feature = "otel")]
pub fn get_subscriber<Sink>(
    name: String,
    env_filter: String,
    sink: Sink,
) -> impl tracing::Subscriber + Send + Sync
where
    Sink: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    use opentelemetry::trace::TracerProvider;

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(env_filter));
    let formatting_layer = BunyanFormattingLayer::new(name.clone(), sink);

    let tracer = opentelemetry::global::tracer_provider().tracer(name);
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::Registry::default()
        .with(env_filter)
        .with(formatting_layer)
        .with(JsonStorageLayer)
        .with(otel_layer)
}

pub fn init_subscriber(subscriber: impl tracing::Subscriber + Send + Sync) {
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}
