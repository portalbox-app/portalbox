use std::env;

use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{prelude::*, EnvFilter};

use crate::config::Config;

pub fn init_subscriber(config: &Config) {
    let local_layer = {
        let default = format!("{}=info", env!("CARGO_CRATE_NAME"));
        let default = default
            .parse()
            .expect("hard-coded default directive should be valid");

        let local_filter = EnvFilter::builder()
            .with_default_directive(default)
            .parse_lossy(&config.log);
        tracing_subscriber::fmt::layer().with_filter(local_filter)
    };

    let telemetry = {
        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint("https://otlp.portalbox.app:4317"),
            )
            .install_batch(opentelemetry::runtime::Tokio)
            .unwrap();

        let filter = if config.telemetry {
            tracing_subscriber::filter::Targets::new()
                .with_target(env!("CARGO_CRATE_NAME"), tracing::Level::INFO)
        } else {
            tracing_subscriber::filter::Targets::new()
        };

        tracing_opentelemetry::layer()
            .with_tracer(tracer)
            .with_filter(filter)
    };

    tracing_subscriber::registry()
        .with(telemetry)
        .with(local_layer)
        .init();
}
