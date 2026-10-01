# Payment engine

A Rust library and CLI for CSV payment processing, built in reviewable phases.

## Current status: Phase 4

The crate and component boundaries are established. The binary validates exactly
one input-path argument. It does not open files or process payments yet. A valid
invocation succeeds with an explicit status message on stderr and empty stdout.

The domain now exposes distinct client (`u16`) and transaction (`u32`) identifiers,
signed `Money`, and strictly positive `PositiveAmount` transaction amounts.
Money uses `i128` units of 0.0001, checked addition/subtraction, and exact decimal
parsing without floating point. Supported balances range from
`-17014118346046923173168730371588410.5728` to
`17014118346046923173168730371588410.5727`.

Decimal input accepts surrounding whitespace, an optional sign, and one to four
fractional digits or an integer. Digits are required before the decimal point
and after it when present. Scientific notation and more than four fractional
digits (including trailing zeros) are rejected. Display always emits four places
and normalizes negative zero. Identifier parsing accepts surrounding whitespace
and the complete underlying unsigned range, including zero.

`Account` provides deposits, withdrawals, holds, releases, and chargebacks with
private balance fields. All operations check arithmetic and the derived total
before committing changes; failures leave balances and lock status unchanged.
Withdrawals use only available funds. Holds may make available funds negative;
releases and chargebacks require sufficient held funds. Chargebacks lock the
account permanently, blocking new deposits and withdrawals while allowing
lifecycle operations. Account methods enforce balance rules only.

Accepted originals are represented by `Transaction`, retaining ID, owner, type,
amount, and lifecycle state. Only deposits can transition from posted to disputed;
resolution returns them to posted, and chargeback is terminal. Repeated or
inapplicable actions return typed transition errors. `transition` returns a new
candidate without mutating the original or touching balances. The manager in the
next phase will verify ownership and commit account and transaction changes
together. Rejected originals must not be constructed as posted transactions. The public
`domain::transaction` module scopes the `State` and `Type` enums; consumers import
them directly or alias them when other domain types would conflict.

```sh
cargo build
cargo run -- transactions.csv
cargo test
```

## Boundaries

Domain modules with unit tests use a directory named after the module, containing
`mod.rs` for implementation and `tests.rs` for tests. Unit tests are included only
under `cfg(test)`. Parameterized cases use `rstest` as a development dependency;
CLI integration tests stay in the top-level `tests` directory.

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
