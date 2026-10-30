//! Tracing and OpenTelemetry export (NFR-13). Set OTEL_EXPORTER_OTLP_ENDPOINT
//! (e.g. http://otel-collector:4318) to export spans; logs go to stdout (JSON with AIPS_LOG_JSON=1).
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

pub struct TelemetryGuard {
    provider: Option<opentelemetry_sdk::trace::TracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(p) = self.provider.take() {
            let _ = p.shutdown();
        }
    }
}

pub fn init(service: &'static str) -> TelemetryGuard {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,neo4rs=warn"));
    let json = std::env::var("AIPS_LOG_JSON")
        .map(|v| v == "1")
        .unwrap_or(false);
    let fmt: Box<dyn Layer<_> + Send + Sync> = if json {
        tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(true)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .compact()
            .boxed()
    };

    let mut provider = None;
    let otel = match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(endpoint) if !endpoint.is_empty() => {
            match opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_endpoint(format!("{}/v1/traces", endpoint.trim_end_matches('/')))
                .build()
            {
                Ok(exporter) => {
                    let p = opentelemetry_sdk::trace::TracerProvider::builder()
                        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
                        .with_resource(opentelemetry_sdk::Resource::new(vec![
                            KeyValue::new("service.name", service),
                            KeyValue::new("service.namespace", "aips"),
                        ]))
                        .build();
                    let tracer = p.tracer(service);
                    opentelemetry::global::set_tracer_provider(p.clone());
                    provider = Some(p);
                    Some(tracing_opentelemetry::layer().with_tracer(tracer))
                }
                Err(e) => {
                    eprintln!("OTel exporter disabled: {e}");
                    None
                }
            }
        }
        _ => None,
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt)
        .with(otel)
        .init();
    TelemetryGuard { provider }
}

/// Request span with method and path only. The query string is never recorded: the UI's
/// server-sent-events requests carry `access_token` there, and spans are exported to OTLP.
pub fn request_span<B>(req: &axum::http::Request<B>) -> tracing::Span {
    tracing::info_span!("request", method = %req.method(), path = %req.uri().path())
}
