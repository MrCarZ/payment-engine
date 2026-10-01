# Payment engine

A Rust library and CLI for CSV payment processing, built in reviewable phases.

## Current status: Phase 13

The binary streams a single CSV synchronously, or validates and processes multiple
CSVs concurrently with disjoint clients. It writes sorted account balances to
stdout and stores each invocation's artifacts under `output/<run-id>/`.
Inputs remain unchanged. Use `--output-dir` to choose another output root.

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
LF and CRLF record endings are supported. Positions refer to the original input
bytes, with one-based physical lines (including newlines inside quoted fields)
and zero-based byte offsets. CR-only record endings are not supported.
Source metadata is shared between records rather than copied per row.

Headers must contain exactly `type`, `client`, `tx`, and `amount`, each once, in any
order. Surrounding field whitespace is trimmed. Monetary rows require positive
amounts; lifecycle rows ignore their amount field and may omit it when it is the
final column. Other field-count mismatches are rejected. Parsing failures identify
the source, position, and invalid field without including the raw row. A header-only
input is valid; an empty file is rejected for missing headers. The CLI uses this
adapter and returns a failure exit status on input errors.

`adapters::payment::csv::output::write` accepts any `Write` destination and an
iterator of `(ClientId, &Account)` snapshots, including `manager.accounts()`.
It sorts by numeric client ID and emits `client,available,held,total,locked` with
four fractional places and lowercase booleans. Empty account sets still produce
the header. Output derives total through checked arithmetic, leaves accounts
unchanged, and explicitly flushes the destination. Serialization, write, flush,
and arithmetic failures propagate through `OutputError`. Sorting retains account
references, not transaction history. Failed writes may leave partial output;
atomic file publication remains the caller's responsibility. The CLI invokes
this adapter after processing and flushing request traces successfully.

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
- `domain/observability`: generic event types.
- `manager/observability`: trace delivery contract.
- `adapters/observability`: CSV delivery and in-memory recording.
- `domain/clock`: shared timestamp contract.
- `adapters/clock`: system clock implementation.
- `bootstrap`: execution configuration, wiring, and lifecycle.
- Binary: process arguments, diagnostics, and exit status.

Components will be constructed explicitly rather than accessed through globals.
Future adapters call managers, which call domain methods. Payment
trace mapping stays outside the shared observability service.

CSV, Serde, JSON, and UTC timestamp dependencies support the current bootstrap.
Account output is reserved for stdout; diagnostics use stderr.

Payment manager implementation, outcomes/reasons, and original-record storage
live in `manager/payment/mod.rs`, with unit tests in `tests.rs`. Requests and
errors live in `request.rs` and `error.rs`, re-exported through the payment module.

Module-local errors for accounts, money, transaction transitions, and bootstrap
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

## Shared tracing

`manager::observability::TraceService` exposes object-safe `emit(Event)` and `flush()`
operations. Event types live in `domain::observability` and contain severity, component, stable event name, optional
correlation ID, message, and a JSON attribute map. Sinks timestamp events through
an injectable `domain::clock::Clock` and normalize timestamps to UTC. `SystemClock` is the
default; tests supply a fixed clock.

`adapters::observability::csv::CsvTraceService` accepts any `Write` destination and emits
`timestamp,severity,component,event_name,correlation_id,message,attributes`.
Timestamps use RFC 3339, severity uses lowercase labels, absent correlation IDs
are empty, and attributes are JSON objects escaped by the CSV writer. Small events
are buffered; successful `emit` does not guarantee delivery. Explicit `flush`
surfaces write/flush failures. Timestamp, JSON, and CSV failures are represented by adapter-local
`CsvTraceError`, wrapped by transport-independent `TraceError` with the original
error preserved through its source chain. The sink does not select files or write to stdout itself.

`adapters::observability::memory::InMemoryTraceService` records timestamped structured events
in order for consumer tests. The shared service contains no payment rules or
partner-specific event mappings. The bootstrap constructs the sink and handles
delivery failures explicitly.

Observability follows the same domain/manager/adapter structure as payment.
The trace contract has no CSV dependencies, and event types depend on neither
managers nor adapters. The shared clock contract belongs to `domain/clock`,
available to any service in the codebase. The default system clock belongs to
`adapters/clock`; concrete trace delivery implementations also belong to adapters.

## Payment trace mapping

`manager::payment::trace` builds events from validated requests, processing
reports, fatal processing errors, and caller-supplied run summaries. These are
pure mappings: the payment manager keeps no trace dependency, and delivery never
invokes payment processing. The caller delivers the returned `Event` through
`TraceService` and handles delivery failures without retrying an applied payment.

Event names are `payment.request_applied`, `payment.request_ignored`,
`payment.request_rejected`, `payment.request_replayed`,
`payment.processing_failed`, and `payment.run_finished`. Business reasons use
explicit snake-case codes suitable for aggregation. Applied events use Info;
ignored/rejected events use Warn; fatal processing/input failures use Error.
Replay events preserve the original outcome and reason while explicitly setting
`replayed=true`, so an earlier success is not another balance movement.

Events identify the run, source, optional partner, client, transaction, and request
type where available. The run ID is the correlation ID. Amounts, balances, raw
rows, and underlying error messages are omitted. Source and client identities
remain trace attributes rather than proposed metric labels.

`adapters::payment::csv::trace` adds record/line/byte provenance to payment
events and maps CSV failures to `payment.input_failed` with stable reason codes
and the invalid field where available. Transport-specific mappings stay in the
adapter; API/webhook adapters can reuse the manager mapping independently.

Run `Summary` counts applied, ignored, rejected, replayed, input errors, and
processing errors. Applied/ignored/rejected exclude replays; the replay count
includes retries of any original outcome. Run state is Completed or Failed,
with Info or Error severity respectively. Run accounting and ordered processing
belong to `manager::payment::run::Coordinator`. Manager execution owns
publication order and final trace delivery; bootstrap wires concrete CSV and
filesystem resources.

## Synchronous execution

`adapters::payment::csv::processing::run` accepts injected input/output streams, source context, and an
object-safe trace service. It processes records in input order, counts outcomes
with replays separately, and continues on business rejections or ignored events.
Input errors, fatal processing failures, and trace delivery failures stop the run.
No payment is retried due to a delivery failure.

Account output begins only after all input has processed successfully and the
request trace buffer has flushed. Both successful and failed runs attempt a
run-summary event and explicit final flush. `RunError` retains the primary
failure, work counts, and additional trace errors. Processing errors identify the
source and CSV position. No accounts are published on processing/input failures.
Output failures may leave partial stdout; a summary-delivery or final-flush
failure may occur after complete account output, and still yields a failure exit
status. A Completed summary describes processing/output completion, not a
guarantee that the final trace flush succeeded. Flush is not durable storage.

The CLI enters through `bootstrap::execute`, which creates a fresh run identity
and delegates invocation handling to `adapters/cli/run`. The adapters create an
exclusive run directory,
opens the input sources, and creates one indexed trace CSV per source there.
Each invocation gets a fresh random UUID v4 run ID, shared by its source
contexts, trace correlation IDs, JSON report, and output directory. Existing inputs and run artifacts are preserved. Setup failures stop
before processing; failures before trace initialization cannot emit a trace
summary. The CLI still attempts a JSON report when processing or setup fails
after artifact storage has been initialized.

```sh
cargo run -- transactions.csv > accounts.csv
```

Success exits with status zero; argument, setup, processing, output, and trace
failures exit with a nonzero status. The CLI derives a UUID v5 source identity from the canonical input path
using the URL namespace and native path bytes and leaves partner identity absent. Library callers can supply their
own run/source/partner context and destinations through `run`. Multiple input
paths select validated concurrent batch execution.

Bootstrap is a single composition module:

```text
bootstrap/
    mod.rs
```

`bootstrap::execute` provides a fresh UUID v4 identity and connects the CLI
invocation and output destination to its adapters. Argument parsing lives in
`adapters/cli/config`; invocation handling lives in `adapters/cli/run`. CSV
resources and source identities live in `adapters/payment/csv/processing`, and
filesystem/report delivery lives in `adapters/artifacts`. The former bootstrap
`config`, `payment`, and `artifacts` module paths have been removed. CSV library
callers import the processing adapter directly; transport-independent callers
use the manager contracts.

`manager::payment::run` owns its `Coordinator`, outcome `Summary`, and
transport-independent failures. Its `Record` and `InputFailure` contracts let
adapters supply validated requests and enrich events with provenance. The CSV
adapter implements these contracts without exposing its trace module. The
coordinator knows no CSV types, readers, writers, or file paths. Non-CSV callers
can use the default payment event mapping. Financial state stays in
`PaymentManager`; adapters own resource setup and delivery.

## Batch preflight contract

`manager::payment::batch::ValidatedBatch` validates a vector of ordered `Source`
snapshots through `TryFrom`. It performs no payment processing. A batch must
contain at least one source; header-only sources are valid. Source identities
must be distinct, all sources must belong to the same run, and each record's
complete context must match its declared source context.

Every client mentioned by any request belongs to exactly one source, including
clients on lifecycle requests that might later be ignored or rejected. Original
transaction IDs may occur repeatedly within their owning source, preserving
normal replay/conflict behavior, but cannot occur in another source. This rule
also covers originals that processing may subsequently reject.

Lifecycle references to an original in another source are rejected even when
their client IDs are disjoint. Validation checks references after collecting
all originals, so the result does not depend on whether the original's source
appears before or after the reference. References absent from the whole batch
remain valid input and retain normal unknown-transaction behavior. Validation
does not reorder rows or turn same-source forward references into later actions.

Typed contract errors identify the client/transaction and both source locations
where applicable. Locations use one-based request ordinals, not physical line
numbers. The validated batch exposes borrowed sources and records; consuming it
transfers the snapshots to execution. Successful validation guarantees partition
isolation, not business acceptance or freedom from arithmetic/delivery failures.

`adapters::payment::csv::processing::batch::validate` fully parses supplied CSV readers and source
contexts before applying the manager contract. CSV errors preserve source, line,
record, and byte provenance. It creates no accounts, output files, traces, or
threads. Parsed requests and positions are retained together so future workers
can process the exact validated snapshot without reopening mutable files.
This preflight API buffers all batch records in memory; single-CSV execution
continues to stream. Bounded-memory batch input is a future extension.


## Concurrent batch execution

`manager::payment::batch::run` executes the validated snapshots in bounded
groups of scoped OS threads. Each source owns its payment manager and trace sink;
workers share no financial state. Rows retain their source order, while ordering
between sources is unspecified. Reports retain input order and aggregate account
output is sorted by client ID. The CLI uses available parallelism, falling back
to one worker; library callers can supply a nonzero worker limit.

CLI inputs are opened and canonicalized to reject duplicate files. Single and
batch runs derive the same UUID v5 for a given canonical path. Paths are not
exposed in source IDs; identity is stable on the same platform. This is
deterministic identification, not encryption. All sources
are parsed and validated before trace initialization or payment processing.
If a worker fails, its active peers finish and later groups are skipped. A worker
panic is reported as a failure. Processing failures suppress aggregate account
output; final trace failures can occur after account output. Every initialized
sink is finalized without retrying financial operations. The worker bound limits
active processing, not buffered input memory or the number of open trace files.

```sh
cargo run -- tests/fixtures/input/payments-a.csv tests/fixtures/input/payments-b.csv
```

## Per-run artifacts

Run these commands from the crate root. `tests/fixtures/input` contains only source
CSVs, `tests/fixtures/expected` contains reference account snapshots, and generated
run files belong to `output`. Large generated inputs and run artifacts
are ignored by Git. The sample generator and fixture instructions are documented
in `tests/fixtures/README.md`.

```text
output/<run-id>/
    accounts.csv
    report.json
    diagnostics.log
    source-0001.trace.csv
    source-0002.trace.csv    (batch runs)
```

The CLI preserves stdout account output while writing a copy to an exclusive
`accounts.partial.csv`. Only a successful processing/output/trace lifecycle
renames it to `accounts.csv`. Failed runs retain the partial file, which may be
empty or contain incomplete output. The report includes run identity, status,
elapsed time, input filenames, outcome counts, and per-source batch results.
`account_file`, `partial_account_file`, and `trace_files` contain filenames
relative to the run directory. `input_files` contains basenames only. File
error descriptions in reports also use basenames. Diagnostics record status, trace paths, and execution errors. Storage
failures can prevent reports or logs from being fully written; these propagate
as CLI failures and do not cause payments to be retried. Publication and flush
provide no durable-storage guarantee.

```sh
cargo run -- --output-dir output tests/fixtures/input/payments-a.csv
cargo run -- --output-dir output tests/fixtures/input/payments-a.csv tests/fixtures/input/payments-b.csv
```

Every invocation creates a new run subdirectory, including failed batch preflight
runs once output storage is initialized. The output directory and trace paths are
printed to stderr. No program logs or traces are written beside input CSVs.

## Manager-owned execution lifecycle

`manager::payment::run::run` accepts an iterator of validated record envelopes,
source context, an injected account `Output`, and a trace service. It owns the
processing/flush/publication order and final summary delivery. `Output::publish`
receives borrowed account snapshots; its implementation chooses representation,
sorting, destination, and delivery completion. Manager lifecycle errors retain
typed input records, input errors, output errors, work counts, and secondary
trace failures without knowing CSV positions or filesystem locations.

`manager::payment::batch::run` executes a `ValidatedBatch<R>` of any sendable
record envelopes with bounded scoped workers. Source reports, cancellation,
panic handling, aggregate counts, publication policy, and final tracing belong
to the manager. Output stays on the calling thread; workers own their source
state and trace sinks. Batch publication errors must be sendable because the
shared lifecycle failure type can cross worker boundaries.

CSV parsing, preflight readers, file opening, source UUID construction, and
trace setup belong to CSV adapters. The CSV output adapter implements the
publication contract; adapter error/report mappings retain CSV provenance.
Artifact adapters own run UUID generation, file storage, and serialized reports.

## Adapter-owned CLI, CSV composition, and artifacts

Argument parsing and invocation configuration live in `adapters/cli/config`.
CSV reader/file composition, preflight reader handling, trace setup, and typed
CSV error/report conversion live in `adapters/payment/csv/processing`. The CSV
output adapter implements the manager's account publication contract. CSV
position metadata is declared in `adapters/payment/csv/position.rs` and exported
through the CSV module.

`adapters/artifacts/filesystem.rs` owns exclusive directory/file creation,
partial account publication, trace discovery, and the file/stdout tee writer.
`adapters/artifacts/report` defines typed serialized reports, source results,
outcome counts, and completion status. Artifact persistence writes JSON and
diagnostics while preserving the existing filename-only report schema.

Bootstrap exposes only its composition entry point and result/error types.
Adapters depend on managers and other adapters, never on bootstrap. CLI
invocation handling and artifact completion live under `adapters/cli/run`; all
financial execution policy remains in managers.
