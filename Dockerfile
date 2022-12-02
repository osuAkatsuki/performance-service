FROM rust:latest as build

RUN USER=root cargo new --bin performance-service
WORKDIR /performance-service

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
COPY ./build.rs ./build.rs

RUN cargo build --release & rm src/*.rs & rm migrations/*.sql

COPY ./src ./src
COPY ./migrations ./migrations

RUN rm ./target/release/deps/performance-service*
RUN cargo build --release

RUN cargo install sqlx-cli --features mysql
RUN sqlx migrate run --ignore-missing

FROM debian:buster-slim

COPY --from=build /performance-service/target/release/performance-service .
CMD ["./performance-service"]
