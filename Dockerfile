# syntax=docker/dockerfile:1.7

# ---------------------------------------------------------------- 1. web UI
FROM node:22-bookworm-slim AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build

# ---------------------------------------------------------------- 2. backend (embeds the UI)
FROM rust:1.99-bookworm AS backend
WORKDIR /app

# Build dependencies first so they are cached between source changes.
COPY Cargo.toml Cargo.lock ./
COPY backend/Cargo.toml backend/Cargo.toml
RUN mkdir -p backend/src && echo "fn main() {}" > backend/src/main.rs \
    && cargo build --release --locked -p odoo-backup \
    && rm -rf backend/src target/release/odoo-backup target/release/deps/odoo_backup-*

COPY backend/ backend/
COPY --from=frontend /app/frontend/dist frontend/dist
RUN cargo build --release --locked -p odoo-backup

# ---------------------------------------------------------------- 3. runtime
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --user-group --home-dir /data --shell /usr/sbin/nologin odoobackup \
    && mkdir -p /data /backups \
    && chown odoobackup:odoobackup /data /backups

COPY --from=backend /app/target/release/odoo-backup /usr/local/bin/odoo-backup

ENV ODB_DATA_DIR=/data \
    ODB_BIND=0.0.0.0:8080 \
    RUST_LOG=info
VOLUME ["/data"]
EXPOSE 8080
USER odoobackup
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 CMD ["odoo-backup", "healthcheck"]
ENTRYPOINT ["odoo-backup"]
CMD ["serve"]
