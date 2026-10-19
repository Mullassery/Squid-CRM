# Build stage
FROM rust:1-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p squid-server

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --uid 10001 --create-home squid
COPY --from=builder /app/target/release/squid-server /usr/local/bin/squid-server

USER squid
EXPOSE 3000
ENV BIND_ADDR=0.0.0.0:3000
CMD ["squid-server"]
