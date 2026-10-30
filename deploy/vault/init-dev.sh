#!/bin/sh
# Development only: seed KV v2 secrets read by aips-core and aips-emergency at start-up.
set -e
until vault status >/dev/null 2>&1; do sleep 1; done
vault kv put secret/aips/core \
  DATABASE_URL="postgres://aips:aips-dev@postgres:5432/aips" \
  NEO4J_URI="neo4j:7687" NEO4J_USER="neo4j" NEO4J_PASSWORD="aips-dev-graph" \
  KAFKA_BROKERS="kafka:9092"
vault kv put secret/aips/emergency EMERGENCY_DATABASE_URL="postgres://aips:aips-dev@postgres:5432/aips_emergency"
echo "vault seeded"
