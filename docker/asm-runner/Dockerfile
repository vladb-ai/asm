# warning: run from repo root, NOT inside this dir
#
# Builds strata-asm-runner with default features. The runner supports three
# prover modes selected by config (see docker/README.md):
#   - none:   `[orchestrator]` omitted entirely
#   - native: `orchestrator.backend.kind = "native"`
#   - sp1:    `orchestrator.backend.kind = "sp1"`  (NOT supported by this image)
#
# TODO(prover-sp1): build an SP1-capable image. The native prover already works
# with this image because it's compiled in unconditionally; SP1 mode requires:
#   1. Swap this builder for an image with the SP1 toolchain (e.g.
#      `FROM ghcr.io/succinctlabs/sp1:...`) so `cargo +succinct` is available.
#   2. Drop `SP1_SKIP_PROGRAM_BUILD=true` and build the guest ELFs via
#      `cargo b -r -p strata-asm-sp1-guest-builder` so `guest-builder/sp1/elfs/{asm,moho}.elf`
#      get produced.
#   3. Build the runner with `--features sp1`.
#   4. Copy the guest ELFs into the runtime image and point
#      `orchestrator.backend.sp1.{asm,moho}_elf_path` at them from config.toml.
# Plumb this through as a separate Dockerfile or behind a build-arg once the
# native flow is settled.
FROM --platform=linux/amd64 ubuntu:24.04 AS builder

WORKDIR /app

ENV CARGO_INCREMENTAL=0
ENV CARGO_TERM_COLOR=always
ENV DEBIAN_FRONTEND=noninteractive
ENV PATH=/root/.cargo/bin:$PATH
# strata-asm-runner's default features don't pull SP1 in, but the guest-builder
# crate's build.rs still gets compiled as part of the workspace metadata pass —
# skip the actual guest compilation so we don't need the SP1 toolchain here.
ENV SP1_SKIP_PROGRAM_BUILD=true

RUN apt-get update && \
    apt-get -y upgrade && \
    apt-get install -y --no-install-recommends \
        pkg-config \
        build-essential \
        protobuf-compiler \
        git \
        curl \
        ca-certificates && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Install rustup with no default toolchain; `rust-toolchain.toml` pins the
# exact version and `rustup show` materializes it.
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain none --profile minimal

COPY rust-toolchain.toml rust-toolchain.toml
RUN rustup show && cargo --version

COPY . .

# Build the binary and lift it out of the cache-mounted target dir so it
# persists into the layer.
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/app/target \
    cargo b -r -p strata-asm-runner && \
    cp /app/target/release/strata-asm-runner /usr/local/bin/strata-asm-runner

FROM --platform=linux/amd64 ubuntu:24.04 AS runtime

WORKDIR /app

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get upgrade -y && \
    apt-get install -y --no-install-recommends \
        ca-certificates && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/strata-asm-runner /usr/local/bin/strata-asm-runner

ENTRYPOINT ["/usr/local/bin/strata-asm-runner"]
CMD ["--config", "/app/config.toml", "--params", "/app/asm-params.json"]
