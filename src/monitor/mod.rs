use std::sync::Arc;
use axum::{
    extract::State,
    response::IntoResponse,
    routing::get,
    Router,
};
use std::sync::atomic::Ordering;
use crate::batching::DynamicBatcher;
use crate::inference::InferenceEngine;

pub struct MonitorService {
    batcher: Arc<DynamicBatcher>,
    engine: Arc<InferenceEngine>,
}

impl MonitorService {
    pub fn new(batcher: Arc<DynamicBatcher>, engine: Arc<InferenceEngine>) -> Arc<Self> {
        Arc::new(Self { batcher, engine })
    }

    pub async fn run(self: Arc<Self>, port: u16) {
        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/metrics", get(metrics_handler))
            .with_state(self);

        println!("Metrics server running on 0.0.0.0:{port}");

        let addr = format!("0.0.0.0:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

        axum::serve(listener, app).await.unwrap();
    }
}

// handlers

async fn health_handler() -> impl IntoResponse {
    "OK"
}

async fn metrics_handler(State(monitor): State<Arc<MonitorService>>) -> impl IntoResponse {
    let m = monitor.batcher.metrics();
    let active = monitor.engine.active_requests.load(std::sync::atomic::Ordering::Relaxed);

    format!(
        "queue_depth {}\n\
     total_batches {}\n\
     total_items {}\n\
     avg_batch_size {:.2}\n\
     queue_wait_ns {}\n\
     infer_latency_ns {}\n\
     batch_latency_ns {}\n\
     active_requests {}\n",
        m.queue_depth.load(std::sync::atomic::Ordering::Relaxed),
        m.total_batches.load(std::sync::atomic::Ordering::Relaxed),
        m.total_items.load(std::sync::atomic::Ordering::Relaxed),
        m.avg_batch_size(),
        m.queue_wait_ns.load(Ordering::Relaxed),
        m.infer_latency_ns.load(Ordering::Relaxed),
        m.batch_latency_ns.load(Ordering::Relaxed),
        active
    )
}
