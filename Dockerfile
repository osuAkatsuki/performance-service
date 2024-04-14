# syntax=docker/dockerfile:1.3-labs

FROM rust:latest AS build

RUN cargo new --lib /performance-service
COPY Cargo.toml Cargo.lock /performance-service/

WORKDIR /performance-service
RUN --mount=type=cache,target=/usr/local/cargo/registry cargo build --release

COPY . /performance-service

RUN --mount=type=cache,target=/usr/local/cargo/registry <<EOF
  set -e
  # update timestamps to force a new build
  touch /performance-service/src/main.rs
  cargo build --release
EOF


FROM debian:bookworm-slim AS runtime
WORKDIR /performance-service

COPY scripts /scripts
COPY migrations /migrations

RUN apt update && apt install -y openssl python3-pip
RUN pip install --break-system-packages -i https://pypi2.akatsuki.gg/cmyui/dev akatsuki-cli

COPY --from=build /performance-service/target/release/performance-service /usr/local/bin
CMD ["/scripts/bootstrap.sh"]
