FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --locked --release --bin xpo-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r xpo \
    && useradd -r -g xpo -d /nonexistent -s /usr/sbin/nologin xpo \
    && mkdir -p /etc/xpo/certs \
    && chown -R xpo:xpo /etc/xpo
COPY --from=builder /app/target/release/xpo-server /usr/local/bin/xpo-server
USER xpo:xpo
EXPOSE 8080 8081
CMD ["/usr/local/bin/xpo-server"]
