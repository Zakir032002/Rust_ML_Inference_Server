#![allow(unused)]

mod inference;
mod grpc;
mod batching;
mod monitor;

use tonic::transport::Server;
use std::{path::Path, sync::Arc};
use anyhow::Result;

use inference::InferenceEngine;
use batching::DynamicBatcher;
use monitor::MonitorService;
use crate::grpc::server::InferenceServer;
use crate::grpc::server::proto::inference_service_server::InferenceServiceServer;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(" Starting RustML Inference Service...");

    // 1. Load engine
    let engine = Arc::new(
        InferenceEngine::new(Path::new("models/resnet50.onnx"))
            .expect(" Failed to initialize inference engine"),
    );

    // 2. Start dynamic batcher
    let max_batch_size = 4;
    let max_wait_ms = 8;
    let batcher = DynamicBatcher::new(engine.clone(), max_batch_size, max_wait_ms);

    // 3. Build GRPC server using the batcher
    let grpc_service = InferenceServer::new(engine.clone(),batcher.clone());

    // 4. Start monitor metrics server
    let monitor = MonitorService::new(batcher.clone(), engine.clone());
    let monitor_task = tokio::spawn(async move {
        monitor.run(3000).await;
    });

    // 5. Start gRPC server (port 50051)
    let addr = "0.0.0.0:50051".parse().unwrap();
    println!(" gRPC server running on {addr}");

    let grpc_task = tokio::spawn(async move {
        Server::builder()
            .add_service(InferenceServiceServer::new(grpc_service))
            .serve(addr)
            .await
            .expect(" gRPC server failed");
    });

    // 6. WAIT FOREVER so main never exits
    tokio::select! {
        _ = grpc_task => {},
        _ = monitor_task => {},
    }

    Ok(())
}
