# Build from the repository root:  docker build -f warehouse/Dockerfile .
FROM python:3.13-slim AS build
WORKDIR /src
COPY orm ./orm
COPY warehouse ./warehouse
RUN python -m venv /opt/venv \
    && /opt/venv/bin/pip install --no-cache-dir ./orm ./warehouse

FROM python:3.13-slim
RUN useradd --system --uid 10001 secretary
COPY --from=build /opt/venv /opt/venv
ENV PATH="/opt/venv/bin:$PATH" \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1
USER secretary
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --retries=5 \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://localhost:8080/healthz', timeout=2)" || exit 1
ENTRYPOINT ["secretary-warehouse"]
