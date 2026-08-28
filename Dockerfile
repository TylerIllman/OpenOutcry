# Two builders, one small runtime. The Rust binary serves the React bundle and
# the WebSocket on one origin, so this image is the entire deployment.

# --- build the front end ----------------------------------------------------
FROM node:22-slim AS web
WORKDIR /app/web
# Copy manifests first so `npm ci` is cached until dependencies actually change.
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# --- build the server -------------------------------------------------------
FROM rust:1.98-slim AS build
WORKDIR /app
# rusqlite's `bundled` feature compiles SQLite from source, which needs a C
# compiler.
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential \
 && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY engine/ engine/
COPY server/ server/
RUN cargo build --release -p server

# --- runtime ----------------------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/target/release/server /usr/local/bin/server
COPY --from=web /app/web/dist ./web/dist

ENV PORT=8080
ENV STATIC_DIR=/app/web/dist
# Overridden in fly.toml to point at the mounted volume.
ENV DB_PATH=/app/open_outcry.db

EXPOSE 8080
CMD ["server"]
