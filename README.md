# chip-as-text

Hardware as Text — define, parse, validate, hash, import, elaborate, and analyze chips and memory systems using controlled natural language.

`chip-as-text` is a Rust library plus CLI for treating CHIP and MEMORY definitions as executable textual specifications.

## What changed in v0.5

This release deepens the compiler front-end shape:

- source spans on parsed structures and diagnostics
- richer connection grammar with optional port-level endpoints
- graph-aware validation warnings
- module and memory block endpoint resolution
- elaborated connection endpoints with kind and port metadata

This is the point where the crate starts behaving more like a small semantic compiler front-end than a plain parser.

## Why

The core idea is simple:

- hardware specs should be readable by humans
- deterministic enough for tooling
- composable through imports
- hashable for auditability
- validated for semantic trust
- diagnosable with source-aware feedback

One parser supports two product surfaces:

- `CHIP`
- `MEMORY`

## Install

```bash
cargo build
cargo test
```

## CLI

```bash
cargo run -- parse examples/blackwell-sm.chip
cargo run -- parse examples/sm-memory.chip
cargo run -- parse examples/blackwell-sm-imported.chip --json
cargo run -- validate examples/blackwell-sm-imported.chip
cargo run -- validate examples/blackwell-sm-ports.chip --json
cargo run -- validate examples/invalid-sm.chip --json
cargo run -- explain examples/blackwell-sm-ports.chip
cargo run -- hash examples/blackwell-sm-imported.chip
```

## Library

```rust
use chip_as_text::{elaborate, parse_file, validate};

let def = parse_file("examples/blackwell-sm-ports.chip")?;
let report = validate(&def);
assert!(report.is_valid);

let elaborated = elaborate(&def)?;
assert_eq!(elaborated.total_instances, 13);
assert_eq!(elaborated.connections[0].from.port.as_deref(), Some("selected_warp"));
# Ok::<(), String>(())
```

## Semantic layer

### Validation

`validate()` returns a structured `ValidationReport` with diagnostics.

Examples of checks:

- duplicate module definitions
- duplicate memory block definitions
- unknown modules in `Instantiate`
- malformed connections
- unknown modules or memory blocks in `Connect`
- invalid source or destination ports on module endpoints
- zero-count instantiation
- empty descriptions or outputs as warnings
- defined-but-never-instantiated modules as warnings
- instantiated-but-never-connected modules as warnings
- disconnected connectivity subgraphs as warnings

Diagnostics are stable structured objects with:

- code
- severity
- message
- section
- subject
- source span

### Elaboration

`elaborate()` resolves the parsed definition into an `ElaboratedDesign`.

That semantic model includes:

- resolved modules
- per-module instance counts
- inbound and outbound connection counts
- resolved connection endpoints
- total instance count
- carried-through canonical hash

## Connection grammar

Connections support both coarse and port-level forms:

```text
Warp Scheduler -> Tensor Core
Warp Scheduler.selected_warp -> Tensor Core.operand_a
Register File.write_value -> Shared Memory.write
```

Notes:

- module endpoints with ports are validated against declared `Outputs` on the source side
- module endpoints with ports are validated against declared `Inputs` on the destination side
- memory block endpoints may optionally carry a port label, but are not yet type-checked

## Format

Supported top-level sections:

- `Full Name:`
- `Description:`
- `Architecture Goals:`
- `Modules:`
- `Instantiate:`
- `Connect:`
- `Memory:`
- `Output:`

Supported headers:

- `# CHIP v2 <name>`
- `# MEMORY v2 <name>`

## Hierarchical imports

Use `IMPORT <relative-path>` to inline reusable fragments before parsing.

Example:

```text
Modules:
IMPORT fragments/warp-scheduler.chipfrag
IMPORT fragments/tensor-core.chipfrag
```

Rules:

- imports are resolved relative to the file that declares them
- imports are recursive
- import cycles are rejected
- imported files should be fragments, not full CHIP or MEMORY documents with their own header

## Canonical hashing

`canonical_hash()` hashes a deterministic canonical serialization of the full parsed structure, including:

- kind
- name
- full name
- description
- goals
- modules
- instances
- connections
- memory blocks
- output

Source spans are intentionally excluded from canonical hashing, so formatting location changes do not alter the semantic hash.

## JSON output

Use the CLI to print pretty JSON for parsed, validated, or elaborated definitions:

```bash
cargo run -- parse examples/blackwell-sm-imported.chip --json
cargo run -- validate examples/invalid-sm.chip --json
cargo run -- explain examples/blackwell-sm-ports.chip --json
```

## License

Dual-licensed under MIT or Apache-2.0.
