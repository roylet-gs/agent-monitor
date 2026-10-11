# ============================================================
# Stage: base — shared Node.js + pnpm + system deps
# ============================================================
FROM node:20-slim AS base

RUN apt-get update && apt-get install -y --no-install-recommends \
    git curl ca-certificates python3 make g++ jq \
  && rm -rf /var/lib/apt/lists/*

RUN corepack enable && corepack prepare pnpm@9 --activate

WORKDIR /work

# ============================================================
# Stage: deps — install project dependencies
# ============================================================
FROM base AS deps

COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

# ============================================================
# Stage: rust-build — compile the native binaries (am-tui, am, am-hook)
# ============================================================
# bookworm to match the runtime stage's glibc.
FROM docker.io/library/rust:1-slim-bookworm AS rust-build

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# E2E doesn't need the shipping profile's fat LTO / single codegen unit, which
# make a cold release build several times slower (and memory-hungry on a 2 GB
# Podman VM). Everything else about the release profile is kept.
ENV CARGO_PROFILE_RELEASE_LTO=false \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
    CARGO_TERM_COLOR=never

# Cache mounts keep the registry and target dir across rebuilds (BuildKit and
# Buildah/Podman both support them). The binaries are copied out because the
# cached target dir isn't part of the image layer.
RUN --mount=type=cache,id=am-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=am-cargo-target,target=/src/target \
    cargo build --release --locked -p am-tui -p am-cli -p am-hook \
  && mkdir -p /out \
  && cp target/release/am-tui target/release/am target/release/am-hook /out/

# ============================================================
# Stage: app — the native TUI (am-tui) served via ttyd
# ============================================================
FROM docker.io/library/debian:bookworm-slim AS app

RUN apt-get update && apt-get install -y --no-install-recommends \
    git curl ca-certificates jq sqlite3 procps \
  && rm -rf /var/lib/apt/lists/*

ARG TTYD_VERSION=1.7.7
RUN ARCH=$(uname -m) \
  && curl -fsSL "https://github.com/tsl0922/ttyd/releases/download/${TTYD_VERSION}/ttyd.${ARCH}" -o /usr/local/bin/ttyd \
  && chmod +x /usr/local/bin/ttyd

COPY --from=rust-build /out/am-tui /out/am /out/am-hook /usr/local/bin/

# Install fake gh on PATH
COPY e2e/bin/gh /usr/local/bin/gh
RUN chmod +x /usr/local/bin/gh

COPY e2e/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENV AM_DATA_DIR=/tmp/am-e2e
ENV AM_MOCK_API_URL=http://mock-api:4100
ENV AM_LINEAR_API_URL=http://mock-api:4100/linear

# 7681: seeded dashboard. 7682: fresh data dir per connection (setup wizard).
EXPOSE 7681 7682
ENTRYPOINT ["/entrypoint.sh"]

# ============================================================
# Stage: test — Playwright tests
# ============================================================
FROM mcr.microsoft.com/playwright:v1.50.0-noble AS test

RUN npm install -g corepack@latest && corepack enable && corepack prepare pnpm@9 --activate

WORKDIR /work

COPY --from=deps /work/node_modules ./node_modules
COPY package.json pnpm-lock.yaml ./

RUN pnpm exec playwright install chromium

# Specs are copied after the browser install so editing a spec doesn't
# invalidate that layer.
COPY e2e/playwright.config.ts ./e2e/
COPY tests/e2e/docker/ ./tests/e2e/docker/

ENV TUI_URL=http://app:7681
ENV MOCK_API_URL=http://mock-api:4100

CMD ["pnpm", "exec", "playwright", "test", "--config", "e2e/playwright.config.ts"]
