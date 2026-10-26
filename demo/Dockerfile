# The hosted demo: Zebra regtest, anvil with the escrow, a house maker and a shared taker, behind
# demo/host.py. Build from the repository root:  docker build -f demo/Dockerfile -t cloakroom-demo .

# ---- the Cloakroom node
FROM rust:1.97-slim-trixie AS node
WORKDIR /src/node
COPY node/Cargo.toml node/Cargo.lock ./
COPY node/src ./src
RUN cargo build --release --locked && strip target/release/cloakroom

# ---- Foundry (forge, cast, anvil), the version the harness and tests use
FROM debian:trixie-slim AS foundry
ARG FOUNDRY=v1.2.3
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
 && curl -fsSL "https://github.com/foundry-rs/foundry/releases/download/${FOUNDRY}/foundry_${FOUNDRY}_linux_amd64.tar.gz" \
    | tar -xz -C /usr/local/bin forge cast anvil

# ---- runtime: the Zebra release image (Debian trixie) plus Python, Foundry and the node
FROM zfnd/zebra:7.0.0-rc.0
USER root
RUN apt-get update && apt-get install -y --no-install-recommends python3 ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=node /src/node/target/release/cloakroom /usr/local/bin/cloakroom
COPY --from=foundry /usr/local/bin/forge /usr/local/bin/cast /usr/local/bin/anvil /usr/local/bin/
WORKDIR /app
COPY --chown=zebra:zebra contracts/foundry.toml contracts/
COPY --chown=zebra:zebra contracts/src contracts/src
COPY --chown=zebra:zebra contracts/lib contracts/lib
COPY --chown=zebra:zebra harness/run.py harness/
COPY --chown=zebra:zebra app/ui app/ui
COPY --chown=zebra:zebra design-system design-system
COPY --chown=zebra:zebra demo demo
RUN mkdir -p harness/work && chown -R zebra:zebra /app
# The image's own unprivileged user (HOME=/home/zebra), which also holds forge's solc cache.
USER zebra
# Compile the contracts now (fetching solc), so start-up only deploys them.
RUN cd contracts && forge build
ENV CLOAKROOM_BIN=/usr/local/bin/cloakroom \
    ZEBRAD=/usr/local/bin/zebrad \
    CLOAKROOM_ZEBRA_CONF=/app/demo/zebrad.toml \
    PORT=8080
EXPOSE 8080
ENTRYPOINT []
CMD ["python3", "-I", "demo/host.py"]
