# ASM Docker Setup

Build from the repository root:

```sh
docker build -f docker/asm-runner/Dockerfile -t strata-asm-runner:latest .
```

The image is a two-stage build: Ubuntu 24.04 with the project's pinned Rust toolchain (from `rust-toolchain.toml`) compiles `strata-asm-runner`, and the binary ships on a slim Ubuntu 24.04 runtime.

## Prover modes

The runner has three prover modes, selected by the config file at runtime:

| Mode   | Config                                              | Supported by this image |
| ------ | --------------------------------------------------- | ----------------------- |
| none   | omit `[orchestrator]`                               | yes                     |
| native | `orchestrator.backend.kind = "native"`              | yes                     |
| sp1    | `orchestrator.backend.kind = "sp1"`                 | no — see TODO           |

SP1 mode needs the binary built with `--features sp1` plus the `asm.elf`/`moho.elf` guest artifacts bundled into the image; the current image is built without SP1. The Dockerfile carries a `TODO(prover-sp1)` with the concrete steps for adding it.

## Running

The default `CMD` points at `/app/config.toml` and `/app/asm-params.json`. The runner doesn't validate the params up front — any misconfiguration surfaces as a runtime failure from the binary itself.

Mount the config and params from the host:

```sh
docker run --rm \
  -v "$PWD/config.toml:/app/config.toml:ro" \
  -v "$PWD/asm-params.json:/app/asm-params.json:ro" \
  -v "$PWD/data:/app/data" \
  -p 9010:9010 \
  strata-asm-runner:latest
```

Override the paths by passing flags directly:

```sh
docker run --rm \
  -v "$PWD/conf:/conf:ro" \
  strata-asm-runner:latest --config /conf/my.toml --params /conf/my.json
```
