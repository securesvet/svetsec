# syntax=docker/dockerfile:1.7

FROM node:24-bookworm AS node-toolchain

FROM rust:1.90-bookworm AS builder
COPY --from=node-toolchain /usr/local/ /usr/local/
ENV RUSTUP_TOOLCHAIN=1.90.0

RUN rustup target add wasm32-unknown-unknown \
    && cargo install --locked trunk@0.21.14

WORKDIR /src
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    npm ci \
    && cargo build --release --locked -p svetsec-server \
    && trunk build --release \
    && install -Dm755 target/release/svetsec-server /out/svetsec-server

FROM node:24-bookworm-slim AS production-dependencies
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --omit=dev

FROM node:24-bookworm-slim AS runtime
ENV NODE_ENV=production \
    SVETSEC_HTTP_ADDR=0.0.0.0:3000 \
    SVETSEC_SSH_ADDR=0.0.0.0:2222 \
    SVETSEC_STATIC_DIR=/app/dist \
    SVETSEC_DATABASE=/data/svetsec.db \
    SVETSEC_PYODIDE_NODE=node \
    SVETSEC_PYODIDE_RUNNER=/app/scripts/pyodide-runner.mjs \
    SVETSEC_PYODIDE_NODE_MODULES=/app/node_modules

WORKDIR /app
COPY --from=builder /out/svetsec-server /usr/local/bin/svetsec-server
COPY --from=builder /src/dist ./dist
COPY --from=production-dependencies /app/node_modules ./node_modules
COPY scripts/pyodide-runner.mjs ./scripts/pyodide-runner.mjs
COPY articles ./articles

EXPOSE 3000 2222
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD node -e "fetch('http://127.0.0.1:3000/api/session').then(r=>{if(!r.ok)process.exit(1)}).catch(()=>process.exit(1))"

CMD ["svetsec-server"]
