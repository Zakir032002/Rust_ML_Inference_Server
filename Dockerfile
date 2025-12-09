# syntax=docker/dockerfile:1
FROM rust:latest as builder
WORKDIR /usr/src/app

# Install protoc (required for tonic/prost)
RUN apt-get update && apt-get install -y protobuf-compiler

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src
RUN echo "fn main() { println!(\"build deps\"); }" > src/main.rs
RUN cargo build --release || true

# Copy project and build
COPY . .
RUN cargo build --release

# --- Final image ---
FROM debian:bookworm-slim
RUN useradd --create-home --home-dir /home/runner -u 1000 runner

WORKDIR /app
COPY --from=builder /usr/src/app/target/release/rustml_inference /app/rustml_inference

USER runner

EXPOSE 50051/tcp
EXPOSE 3000/tcp

CMD ["./rustml_inference"]

