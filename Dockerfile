FROM lukemathwalker/cargo-chef:latest-rust-bookworm AS chef
WORKDIR /performance-service

FROM chef AS prepare
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cook
COPY --from=prepare /performance-service/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin performance-service

FROM debian:bookworm-slim AS runtime
WORKDIR /performance-service

COPY scripts /scripts
COPY migrations /migrations

RUN apt update && apt install -y openssl python3-pip

COPY --from=cook /performance-service/target/release/performance-service /usr/local/bin
CMD ["/scripts/bootstrap.sh"]
