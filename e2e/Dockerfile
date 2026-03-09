# ============================================================
# Stage: base — shared Node.js + pnpm + system deps
# ============================================================
FROM node:20-slim AS base

RUN apt-get update && apt-get install -y --no-install-recommends \
    git curl ca-certificates python3 make g++ \
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
# Stage: app — the real TUI served via ttyd
# ============================================================
FROM base AS app

# Install ttyd
ARG TTYD_VERSION=1.7.7
RUN curl -fsSL "https://github.com/nicktehrany/ttyd/releases/download/${TTYD_VERSION}/ttyd.x86_64" -o /usr/local/bin/ttyd \
  && chmod +x /usr/local/bin/ttyd

COPY --from=deps /work/node_modules ./node_modules
COPY package.json pnpm-lock.yaml tsconfig.json ./
COPY src/ ./src/

# Install fake gh on PATH
COPY e2e/bin/gh /usr/local/bin/gh
RUN chmod +x /usr/local/bin/gh

COPY e2e/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENV AM_DATA_DIR=/tmp/am-e2e
ENV AM_MOCK_API_URL=http://mock-api:4100

EXPOSE 7681
ENTRYPOINT ["/entrypoint.sh"]

# ============================================================
# Stage: test — Playwright tests
# ============================================================
FROM mcr.microsoft.com/playwright:v1.50.0-noble AS test

RUN corepack enable && corepack prepare pnpm@9 --activate

WORKDIR /work

COPY --from=deps /work/node_modules ./node_modules
COPY package.json pnpm-lock.yaml tsconfig.json ./
COPY e2e/playwright.config.ts ./e2e/
COPY tests/e2e/docker/ ./tests/e2e/docker/

RUN npx playwright install chromium

ENV TUI_URL=http://app:7681
ENV MOCK_API_URL=http://mock-api:4100

CMD ["npx", "playwright", "test", "--config", "e2e/playwright.config.ts"]
