# aips-core: Rust service + C++ signal library + ONNX Runtime + AIPS Command UI (served statically).
# Base images are pinned by digest (tag kept for readability); refresh both together.
# Build: docker build -f deploy/docker/core.Dockerfile -t aips/core:0.1.0 .
FROM node:22-bookworm-slim@sha256:4b150b48b4116e2a22f2c39cc27025fbf4f153e2078fd3b14d159fdb5c7279da AS ui
WORKDIR /src/apps/command
COPY apps/command/package.json apps/command/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY apps/command/ ./
RUN npm run build

FROM rust:1.89-bookworm@sha256:948f9b08a66e7fe01b03a98ef1c7568292e07ec2e4fe90d88c07bb14563c84ff AS build
RUN apt-get update && apt-get install -y --no-install-recommends cmake libcurl4-openssl-dev libsasl2-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
ARG ORT_VERSION=1.20.1
RUN mkdir -p /opt/ort && curl -fsSL "https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-linux-x64-${ORT_VERSION}.tgz" \
    | tar xz -C /opt/ort --strip-components=1
ENV AIPS_ONNXRUNTIME_ROOT=/opt/ort
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY services services
COPY libs libs
COPY db db
RUN cargo build --release --locked -p aips-core

FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libcurl4 libsasl2-2 \
    && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10001 --home /nonexistent aips
COPY --from=build /opt/ort/lib/libonnxruntime.so* /usr/local/lib/
COPY --from=build /src/target/release/aips-core /usr/local/bin/aips-core
COPY --from=ui /src/apps/command/dist /srv/aips/ui
COPY ml/models /srv/aips/models
RUN ldconfig
# The registry path is absolute: the image has no WORKDIR, so the relative default never resolved.
ENV AIPS_UI_DIR=/srv/aips/ui AIPS_MODEL_REGISTRY=/srv/aips/models/registry \
    AIPS_CORE_ADDR=0.0.0.0:8090 AIPS_LOG_JSON=1
USER 10001
EXPOSE 8090
# bash, not sh: Debian's sh (dash) has no /dev/tcp, so the check always failed.
HEALTHCHECK --interval=15s --timeout=3s CMD ["/bin/bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/8090 && printf 'GET /livez HTTP/1.0\\r\\n\\r\\n' >&3 && grep -q ok <&3"]
ENTRYPOINT ["/usr/local/bin/aips-core"]
