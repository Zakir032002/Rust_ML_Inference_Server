# RustML Inference Server

A high-performance, production-grade machine learning inference server built in Rust.
This project demonstrates how to deploy ONNX models using techniques common in industrial inference systems, including dynamic batching, session pooling, backpressure, and gRPC endpoints.

The architecture is model-agnostic and can be reused for any deep learning model exported to ONNX (vision, NLP, audio, multimodal, etc.). The system is optimized for CPU inference and works from local development to containerized deployments.

## Features

- gRPC API for sending inference requests
- Dynamic batching to group smaller requests into larger model runs
- Parallel session pool for multi-core CPU utilization
- Backpressure to prevent overload under high traffic
- Axum metrics endpoint for monitoring
- ONNX Runtime for executing neural network models
- Dockerfile for reproducible deployments

Although the demo uses ResNet-50, the architecture is universal and can support any ONNX model with minimal changes.

## Architecture

High-level flow:

Client → gRPC Server → Dynamic Batcher → Session Pool → ONNX Runtime → Response

## Components

1. Inference Engine
   - Loads an ONNX model and creates a pool of inference sessions sized to CPU cores
   - Round-robin selection distributes load evenly across sessions
   - Each session runs ONNX Runtime with tuned CPU threading

2. Dynamic Batcher
   - Collects incoming requests and combines them into batches
   - Batch size and wait time are configurable
   - Improves throughput for CNNs, transformers, and most feed-forward architectures

3. Backpressure
   - Rejects requests when too many are in flight
   - Preserves latency under traffic spikes

4. gRPC Layer
   - Strongly typed interface for inference
   - Clients generate code from the `.proto` file in many languages

5. Metrics Server
   - Axum endpoint exposing internal metrics: queue depth, batch sizes, total items, and latency stats

6. Docker Support
   - Multi-stage build creates a minimal deployment image

## Project Structure
rustml-inference/
├── build.rs
├── Cargo.toml
├── Dockerfile
├── data.json
├── models/
│   └── resnet50.onnx
├── proto/
│   └── inference.proto
└── src/
    ├── main.rs
    ├── inference/
    │   └── mod.rs
    ├── batching/
    │   └── mod.rs
    ├── grpc/
    │   └── server.rs
    └── monitor/
        ├── mod.rs
        └── prometheus.rs

## Running the Server
1. Build the Docker Image
docker build -t rustml-inference:local .

2. Run the Service
docker run --rm \
    -p 50051:50051 \
    -p 3000:3000 \
    -v $(pwd)/models:/app/models \
    rustml-inference:local

3. Service Endpoints

gRPC Inference Service: localhost:50051

Metrics HTTP server: localhost:3000/metrics

Health check: localhost:3000/health

Example Client Usage (Python)

Generate client code:

python -m grpc_tools.protoc \
    -I./proto \
    --python_out=. \
    --grpc_python_out=. \
    proto/inference.proto


Call the inference RPC:

import grpc
import inference_pb2
import inference_pb2_grpc

channel = grpc.insecure_channel("localhost:50051")
stub = inference_pb2_grpc.InferenceServiceStub(channel)

req = inference_pb2.PredictRequest(
    data=[0.0] * (1 * 3 * 224 * 224),
    shape=[1,3,224,224]
)

res = stub.Predict(req)
print(res.shape)
print(res.predictions[:10])

Load Testing

Using ghz:

ghz --insecure \
    --proto proto/inference.proto \
    --call inference.InferenceService.Predict \
    -D data.json \
    -c 2 \
    -n 5 \
    localhost:50051

## Future Extensions

Possible enhancements for a production deployment include:

GPU support (CUDA EP, TensorRT EP)

Prometheus exporters with Grafana dashboards

Distributed autoscaling (Docker Compose or Kubernetes)

Authentication and request signing

Model registry and hot-swappable models

Pre-processing and post-processing pipelines

Streaming RPC for audio/speech models

## Conclusion

This project demonstrates how modern machine learning inference servers are designed in practice.
By incorporating batching, pooling, backpressure, and efficient model execution, it provides a strong foundation for deploying ONNX models at scale.

The architecture is intentionally universal so that different models can be served with minimal changes, making it suitable for both research and real-world production environments.