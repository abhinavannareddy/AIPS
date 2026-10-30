# aips-workflow: Temporal worker (TypeScript) for durable case SLA escalation.
FROM node:22-bookworm-slim@sha256:4b150b48b4116e2a22f2c39cc27025fbf4f153e2078fd3b14d159fdb5c7279da AS build
WORKDIR /app
COPY workers/temporal/package.json workers/temporal/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY workers/temporal/ ./
RUN npm run build && npm prune --omit=dev

FROM node:22-bookworm-slim@sha256:4b150b48b4116e2a22f2c39cc27025fbf4f153e2078fd3b14d159fdb5c7279da
WORKDIR /app
COPY --from=build /app/node_modules node_modules
COPY --from=build /app/dist dist
COPY --from=build /app/package.json .
USER node
ENTRYPOINT ["node", "dist/worker.js"]
