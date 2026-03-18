# Build stage: install deps and build Astro static output.
# Expects build context = repo root (..) so demo/frontend and graphics/ are available.
# Symlinks public/logo.svg and public/m.svg -> ../../../graphics/* resolve to /graphics/*.
FROM node:alpine@sha256:5209bcaca9836eb3448b650396213dbe9d9a34d31840c2ae1f206cb2986a8543 AS builder

RUN corepack enable 2>/dev/null || npm install -g pnpm

WORKDIR /app

# When building for Docker demo, frontend uses /api/martin for tiles/metrics (proxied to tiles service)
ARG PUBLIC_MARTIN_BASE_URL=/api/martin
ENV PUBLIC_MARTIN_BASE_URL=${PUBLIC_MARTIN_BASE_URL}

COPY demo/frontend/package.json demo/frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY demo/frontend .
# Satisfy symlinks public/logo.svg, public/m.svg -> ../../../graphics/* (resolves to /graphics/*)
COPY graphics /graphics
RUN pnpm build

# Production stage: serve dist/ with nginx
FROM nginx:alpine@sha256:f46cb72c7df02710e693e863a983ac42f6a9579058a59a35f1ae36c9958e4ce0

COPY --from=builder /app/dist /usr/share/nginx/html
COPY demo/frontend/nginx.conf /etc/nginx/conf.d/default.conf

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
