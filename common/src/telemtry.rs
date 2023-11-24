use tracing_bunyan_formatter::BunyanFormattingLayer;
use tracing_bunyan_formatter::JsonStorageLayer;
use tracing_subscriber::{fmt::MakeWriter, prelude::__tracing_subscriber_SubscriberExt};

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
    let formatting_layer = BunyanFormattingLayer::new(name.into(), sink);
    tracing_subscriber::Registry::default()
        .with(env_filter)
        .with(formatting_layer)
        .with(JsonStorageLayer)
}

pub fn init_subscriber(subscriber: impl tracing::Subscriber + Send + Sync) {
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}
