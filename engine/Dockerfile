# Build from the repository root:  docker build -f engine/Dockerfile .
FROM rust:1-slim-trixie AS build
WORKDIR /src
# The whole Cargo workspace must be present to resolve it; only the engine is built.
COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY engine ./engine
COPY agent/daemon ./agent/daemon
COPY agent/panel ./agent/panel
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p secretary-engine \
    && cp target/release/secretary-engine /usr/local/bin/

FROM debian:trixie-slim
RUN useradd --system --uid 10001 secretary
COPY --from=build /usr/local/bin/secretary-engine /usr/local/bin/secretary-engine
USER secretary
ENV ENGINE_HEALTH_BIND=0.0.0.0:8070
EXPOSE 8070
# No curl in the image: ask the health port with bash's /dev/tcp.
HEALTHCHECK --interval=10s --timeout=3s --retries=5 \
    CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/8070 && printf "GET /readyz HTTP/1.0\r\n\r\n" >&3 && grep -q "200 OK" <&3' || exit 1
ENTRYPOINT ["secretary-engine"]
