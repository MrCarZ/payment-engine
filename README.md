# Payment engine

A Rust library and CLI for CSV payment processing, built in reviewable phases.

## Current status: Phase 7

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
candidate without mutating the original or touching balances. The payment manager verifies ownership and commits account and transaction changes
together. Rejected originals must not be constructed as posted transactions. The public
`domain::payment::transaction` module scopes the `State` and `Type` enums; consumers import
them directly or alias them when other domain types would conflict.

`manager::payment` exposes `PaymentManager`, scoped `Request`,
`Outcome`, and `Reason` enums, and fatal `ProcessingError` failures. The manager
owns accounts and original records, including rejected originals. It calculates
candidate account and transaction states before committing either. Valid ignored
or rejected requests referencing an unseen client create a zero-balance account.
Arithmetic failures leave account and original records unchanged. Identical
original replays return their stored outcome without applying financial changes.
Conflicting original-ID reuse is rejected. Lifecycle references use the stored
amount and never replace the original processing outcome.

`process` returns a `Report` exposing `outcome()` and `is_replay()`. A replayed
applied outcome means the original succeeded previously; it does not indicate a
new balance movement. Equality compares normalized client, ID, type, and money
values. Replays preserve current transaction state even after chargeback, and a
rejected withdrawal stays rejected after later deposits. Valid business outcomes
reserve original IDs; fatal arithmetic failures do not. Replay detection applies
only to originals, while repeated lifecycle events follow their state rules.

`adapters::payment::csv::input::Input` streams any `Read` source into requests plus processing
context. It reuses a row buffer, preserves input order, and stops permanently after
yielding the first parse or read error. The caller supplies run/source/optional
partner IDs through transport-independent processing context. CSV records and
errors separately carry record index (header is zero), line, and byte offset.
Source metadata is shared between records rather than copied per row.

Headers must contain exactly `type`, `client`, `tx`, and `amount`, each once, in any
order. Surrounding field whitespace is trimmed. Monetary rows require positive
amounts; lifecycle rows ignore their amount field and may omit it when it is the
final column. Other field-count mismatches are rejected. Parsing failures identify
the source, position, and invalid field without including the raw row. A header-only
input is valid; an empty file is rejected for missing headers. The CLI is not yet
wired to this adapter; that integration belongs to Phase 11.

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

- `domain/payment`: payment types and invariants; no CSV or tracing dependencies.
- `manager`: payment requests and coordination of domain operations.
- `adapters/payment`: service-scoped external representations and I/O.
- `observability`: generic structured trace delivery.
- `runtime`: execution configuration, wiring, and lifecycle.
- Binary: process arguments, diagnostics, and exit status.

Components will be constructed explicitly rather than accessed through globals.
Future adapters call managers, which call domain methods. Payment
trace mapping stays outside the shared observability service.

CSV, Serde, JSON, and UTC timestamp dependencies are declared for later phases.
Account output is reserved for stdout; diagnostics use stderr.

Payment manager implementation, outcomes/reasons, and original-record storage
live in `manager/payment/mod.rs`, with unit tests in `tests.rs`. Requests and
errors live in `request.rs` and `error.rs`, re-exported through the payment module.

Module-local errors for accounts, money, transaction transitions, and runtime
arguments live in each module's `error.rs`, re-exported through its `mod.rs`.
Identifier parsing retains the standard library `ParseIntError`.

Payment domain modules are grouped under `domain/payment`, matching the
`manager/payment` boundary. Consumers import payment types from `domain::payment`.

Payment adapters are grouped by service and transport under `adapters/payment/csv`.
The borrowed `row::Row` representation converts into manager `Request` through
`TryFrom`; it requires neither a reader nor processing context. The streaming
input adapter owns headers, record lengths, source positions, and read failures,
then delegates field validation to this conversion. Future API and webhook
representations can independently target the same manager request boundary.
