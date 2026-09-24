# Builds the same non-lambda binary `make run`/`cargo run` already runs
# locally (no --features lambda) for the always-on ECS Fargate task. See
# `dev-docs/phase/14.md`.

FROM rust:1-slim AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY main.rs ./
COPY lib ./lib
RUN cargo build --release --locked --bin omoffice-api

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home omoffice
COPY --from=builder /build/target/release/omoffice-api /usr/local/bin/omoffice-api
USER omoffice
EXPOSE 8000
ENTRYPOINT ["/usr/local/bin/omoffice-api"]
