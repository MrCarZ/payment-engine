# Payment engine

A Rust library and CLI for CSV payment processing, built in reviewable phases.

## Current status: Phase 1

The crate and component boundaries are established. The binary validates exactly
one input-path argument. It does not open files or process payments yet. A valid
invocation succeeds with an explicit status message on stderr and empty stdout.

```sh
cargo build
cargo run -- transactions.csv
cargo test
```

## Boundaries

- `domain`: financial types and invariants; no CSV or tracing dependencies.
- `application`: payment requests and coordination of domain operations.
- `adapters`: external input/output formats.
- `observability`: generic structured trace delivery.
- `runtime`: execution configuration, wiring, and lifecycle.
- Binary: process arguments, diagnostics, and exit status.

Components will be constructed explicitly rather than accessed through globals.
Future adapters call the application layer, which calls domain methods. Payment
trace mapping stays outside the shared observability service.

CSV, Serde, JSON, and UTC timestamp dependencies are declared for later phases.
Account output is reserved for stdout; diagnostics use stderr.
