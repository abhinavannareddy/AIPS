# aips-emergency: Guardian emergency workflow service (separate deployment and data store, EMG-01).
FROM rust:1.89-bookworm@sha256:948f9b08a66e7fe01b03a98ef1c7568292e07ec2e4fe90d88c07bb14563c84ff AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY services services
COPY libs libs
COPY db db
RUN cargo build --release --locked -p aips-emergency

FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10002 --home /nonexistent aips && mkdir -p /var/lib/aips/emergency && chown 10002 /var/lib/aips/emergency
COPY --from=build /src/target/release/aips-emergency /usr/local/bin/aips-emergency
ENV AIPS_EMERGENCY_DATA=/var/lib/aips/emergency AIPS_EMERGENCY_ADDR=0.0.0.0:8091 AIPS_LOG_JSON=1 AIPS_EMERGENCY_LIVE_ENABLED=false
USER 10002
VOLUME /var/lib/aips/emergency
EXPOSE 8091
ENTRYPOINT ["/usr/local/bin/aips-emergency"]
