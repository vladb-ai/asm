# dbtool

Offline inspection and maintenance for ASM storage — the ASM counterpart to
alpen's `strata-dbtool`, but built in a layered `<domain> <resource> <verb>`
grammar instead of a flat verb-prefixed surface.

The binary is `dbtool` (crate `asm-dbtool`).

## Storage model

The runner persists into two independent sled databases:

- **Storage DB** — anchor state, aux data, full manifests, the manifest-hash
  MMR, and the per-container export-entry MMR. Backed by `asm-storage` and
  `strata-asm-moho-storage` (`SledExportEntriesDb`). Targeted by `asm` and
  `moho export-entries` commands.
- **Proof DB** — Moho state snapshots, ASM/Moho proofs, and the remote-prover
  bookkeeping. Backed by `strata-asm-moho-storage` (`SledMohoStateDb`) and
  `strata-asm-prover-storage`. Targeted by `moho state` and the planned `proof`
  commands.

Each invocation opens exactly the one database its command needs, so both are
selected with a single `--db <path>` flag — point it at whichever directory the
command operates on. **sled takes an exclusive lock on the directory, so the
runner must be stopped** while `dbtool` runs.

## Usage

```
dbtool [--db <path>] [--pretty] [--write] <domain> <resource> <verb> [args]
```

- Output is JSON on stdout (compact; `--pretty` for indented). Errors go to
  stderr.
- **Read-only by default.** Mutating verbs (`put`, `delete`, `prune`,
  `put-leaf`) refuse to run without `--write`.
- A commitment argument is written `<height>:<blkid_hex>`. Each record prints
  that exact string in its `block.commitment` field, so a printed commitment
  copies straight into `get`/`delete` (alongside the structured `height` and
  `blkid`).
- Records are SSZ-encoded; each `get` prints the fields we can cheaply decode
  plus an `ssz_hex` blob carrying the canonical bytes losslessly. `put` consumes
  those same bytes from `--file` (raw SSZ, not the hex text), so get → put
  round-trips once you hex-decode `ssz_hex` back to bytes — see the round-trip
  example below.

### Examples

```sh
# Highest anchor state, pretty-printed
dbtool --db ./data/asm --pretty asm state latest

# A manifest and its logs
dbtool --db ./data/asm asm manifest get 1234:6f1a...ee

# List every stored manifest commitment
dbtool --db ./data/asm asm manifest list

# Manifest-hash MMR: count, a leaf, and an inclusion proof
dbtool --db ./data/asm asm manifest-mmr count
dbtool --db ./data/asm asm manifest-mmr leaf 1234
dbtool --db ./data/asm asm manifest-mmr proof 1234 --at 2000

# Roll storage back to a known-good height (mutating → needs --write)
dbtool --db ./data/asm --write asm state prune --after 1234

# Round-trip a record: get → hex-decode its ssz_hex to raw bytes → put it back
dbtool --db ./data/asm asm manifest get 1234:6f1a...ee \
  | jq -r .ssz_hex | xxd -r -p > manifest.ssz
dbtool --db ./data/asm --write asm manifest put --file manifest.ssz

# Moho state (proof DB): highest snapshot and one by commitment
dbtool --db ./data/proof --pretty moho state latest
dbtool --db ./data/proof moho state get 1234:6f1a...ee

# Export-entry MMR (storage DB): a container's size, a leaf, and its proof
dbtool --db ./data/asm moho export-entries count 0
dbtool --db ./data/asm moho export-entries proof 0 5 --at 100
```

## Command surface

### `asm` (storage DB) — implemented

| Resource | Verbs |
|---|---|
| `asm state` | `get <commitment>` · `latest` · `list` · `put --file F` (w) · `delete <commitment>` (w) · `prune (--before\|--after) <h>` (w) |
| `asm aux` | `get <commitment>` · `list` · `put <commitment> --file F` (w) · `delete <commitment>` (w) · `prune (--before\|--after) <h>` (w) |
| `asm manifest` | `get <commitment>` · `list` · `put --file F` (w) · `delete <commitment>` (w) · `prune (--before\|--after) <h>` (w) |
| `asm manifest-mmr` | `count` · `leaf <index>` · `proof <index> [--at <leaf_count>]` · `put-leaf <height> <hash_hex>` (w) |

`(w)` = mutation, gated behind `--write`. The manifest-hash MMR is
height-indexed, so the `<index>` read by `leaf`/`proof` and the `<height>`
written by `put-leaf` are the same value — the leaf for the block at height `h`
is leaf index `h`.

### `moho` — implemented

The two resources live in different databases, so `--db` points at a different
directory for each: `moho state` at the proof DB, `moho export-entries` at the
storage DB.

| Resource | DB | Verbs |
|---|---|---|
| `moho state` | proof | `get <commitment>` · `latest` · `list` · `put <commitment> --file F` (w) · `delete <commitment>` (w) · `prune (--before\|--after) <h>` (w) |
| `moho export-entries` | storage | `get <container> <index>` · `find <container> <hash>` · `height <container> <index>` · `count <container>` · `range <container> <height>` · `proof <container> <index> [--at <n>]` · `append <container> <height> --file F` (w) · `prune --from <height>` (w) |

`moho state put` takes the commitment explicitly — a `MohoState` does not carry
its own key. `moho export-entries` addresses each leaf by its `mmr_index`
within a container; `append` reads a file of concatenated raw 32-byte hashes,
and `prune --from` drops every leaf at or above a height across all containers.

### Planned (proof DB) — not yet implemented

These share the proof DB and the `strata-asm-prover-storage` crate and land in a
follow-up:

- `proof asm get/list/delete` (ASM step proofs)
- `proof moho get/list/latest/delete` (Moho recursive proofs)
- `proof mapping` · `proof status` · `proof prune` (remote-prover bookkeeping)
