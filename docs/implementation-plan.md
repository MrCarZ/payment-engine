# Payment engine implementation plan

This plan records the implemented phases and their review boundaries. It was
reconstructed from the existing implementation and phased development history;
the original planning file was unavailable. Completed implementation does not
replace partner review.

## Implementation phases

1. **Crate foundation — complete.** Library/binary entry points and initial
   module boundaries. Validate compilation before adding payment behavior.
2. **Money and identifiers — complete.** Exact decimal representation, checked
   arithmetic, positive amounts, and typed client/transaction IDs. Validate
   parsing, numeric boundaries, and conversion behavior independently.
3. **Accounts — complete.** Available/held balances, locking, and atomic account
   operations. Validate financial operations without transaction coordination.
4. **Transactions — complete.** Accepted original records and lifecycle
   transitions. Validate transition rules without account mutations.
5. **Payment manager — complete.** Coordinate ownership checks and atomic
   account/transaction changes. Validate interleaved clients and business outcomes.
6. **Original replay handling — complete.** Retain accepted/rejected outcomes,
   detect identical originals and conflicting IDs. Validate absence of repeated
   balance movement and distinguish lifecycle retries from original replays.
7. **CSV input — complete.** Convert streamed rows to manager requests and retain
   record provenance. Validate headers, fields, terminal errors, and reader failures.
8. **CSV account output — complete.** Serialize sorted snapshots with exact
   decimals. Validate formatting and publication failures independently.
9. **Shared observability — complete.** Generic structured events, injected clock,
   trace interface, CSV delivery, and in-memory test sink. Validate buffering and
   explicit flushing independently of payment rules.
10. **Payment trace mapping — complete.** Map outcomes/errors to stable events
    and safe provenance. Validate classifications, correlations, and replay counts.
11. **Single-run composition — complete.** Manager-owned lifecycle, CLI parsing,
    CSV composition, artifact storage, and thin bootstrap. Validate CLI behavior,
    account-only stdout, and failure propagation through integration tests.
12. **Batch preflight — complete.** Validate contexts, disjoint clients/original
    transaction IDs, and cross-source references before processing any source.
13. **Concurrent batch execution — complete.** Bounded source workers, isolated
    managers/sinks, deterministic report order, failure cancellation, and aggregate
    publication. Validate overlap, worker counts, peer completion, and failures.
14. **Final verification and documentation — implemented; awaiting review.**
    Run the existing checks locally, document verification commands and
    operational limits, and retain this plan in version control. This phase adds
    no new test suites or CI infrastructure.

The completed structural refinements place financial orchestration in
`manager/payment`, external composition in `adapters`, and only wiring in
`bootstrap`. Shared clock and observability types remain outside payment scope.

## Final review steps

1. **CSV provenance — complete.** LF/CRLF regression tests cover physical lines,
   original byte offsets, multiline quoted fields, split reads, and trace events.
2. **Failure handling and reporting — complete.** Retain sinks after failed
   worker starts, attempt independent final trace operations despite panics,
   preserve primary errors, and publish only complete JSON reports.
3. **Final verification and documentation — implemented; awaiting review.**
   Run formatting, Clippy, existing tests, documentation tests, and a release
   build locally. Restore the phased plan and document remaining limits without
   adding behavior, test suites, or CI infrastructure.

## Verification

Run from the crate root. Local verification uses Rust 1.98.1 on Windows:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --doc
cargo build --locked --release
```

Compare single and batch CLI output against the fixtures described in
`tests/fixtures/README.md`. Other platforms and toolchain versions have not been
validated in this review. No minimum supported Rust version is asserted.

## Explicit limits and follow-up work

- State and replay records are in memory and scoped to one processing run.
  Cross-run retries, durable recovery, and shared account ownership require a
  separately designed persistence/idempotency contract.
- Batch validation buffers all records and opens all input sources; trace sinks
  are initialized upfront. Worker limits bound execution only. Input/source limits
  and disk-backed preflight remain follow-up work.
- CSV composition currently accepts CLI configuration types. Moving its options
  to a CSV-specific boundary is a future refactor before introducing another
  transport; it is not required for the current CLI contract.
- Disjoint clients and original IDs are enforced for batch files. Supporting
  overlapping clients requires an explicit ordering/partition ownership contract.
- LF/CRLF are supported; CR-only record endings are outside the current contract.
- File flush/rename does not guarantee durability or an atomic transaction across
  stdout, account output, traces, and reports. Later failures retain earlier work
  and never retry financial operations automatically.
- CSV observability delivery is implemented; external tracing providers and
  operational monitoring remain future integrations.
