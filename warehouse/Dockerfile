# Build from the repository root:  docker build -f warehouse/Dockerfile .
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p secretary-warehouse \
    && cp target/release/secretary-warehouse /usr/local/bin/

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 secretary
COPY --from=build /usr/local/bin/secretary-warehouse /usr/local/bin/secretary-warehouse
USER secretary
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --retries=5 CMD curl -fsS http://localhost:8080/healthz || exit 1
ENTRYPOINT ["secretary-warehouse"]
