# Build from the repository root:  docker build -f overview/Dockerfile .
# One image: the dashboard API serving the built web app (same origin, so the
# session cookie and CSRF need no CORS).

FROM node:24-slim AS web
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
WORKDIR /src
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY overview/web/package.json overview/web/
RUN corepack pnpm@10.20.0 install --frozen-lockfile --filter @secretary/overview-web
COPY overview/web overview/web
RUN corepack pnpm@10.20.0 --filter @secretary/overview-web build

FROM python:3.13-slim AS build
WORKDIR /src
COPY orm ./orm
COPY warehouse ./warehouse
COPY overview/api ./overview/api
RUN python -m venv /opt/venv \
    && /opt/venv/bin/pip install --no-cache-dir ./orm ./warehouse ./overview/api

FROM python:3.13-slim
RUN useradd --system --uid 10001 secretary
COPY --from=build /opt/venv /opt/venv
COPY --from=web /src/overview/web/dist /app/web
ENV PATH="/opt/venv/bin:$PATH" \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    STATIC_DIR=/app/web
USER secretary
EXPOSE 8090
HEALTHCHECK --interval=10s --timeout=3s --retries=5 \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://localhost:8090/healthz', timeout=2)" || exit 1
ENTRYPOINT ["secretary-overview"]
