use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

// Returns a handle that axum will expose at /metrics
pub fn init_prometheus() -> PrometheusHandle {
    PrometheusBuilder::new()
        .with_default_registry()
        .install_recorder()
        .expect("failed to install Prometheus recorder")
}

