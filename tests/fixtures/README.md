# Mock payment inputs

Run these commands from the crate root (`payment-engine`). All data is synthetic.

Single CSV:

```sh
cargo run -- tests/fixtures/input/payments-a.csv
```

Expected account output: `tests/fixtures/expected/single-accounts.csv`.
There are 10 applied requests, 1 replay, 2 rejections, and 1 ignored request.

Concurrent batch:

```sh
cargo run -- tests/fixtures/input/payments-a.csv tests/fixtures/input/payments-b.csv
```

Expected account output: `tests/fixtures/expected/batch-accounts.csv`.
Combined counts are 14 applied requests, 2 replays, 3 rejections, and 2 ignored
requests. Worker concurrency uses the process's available parallelism.

Both commands should exit successfully. Business rejections and ignored requests
are recorded in trace CSVs and do not fail execution. Each invocation creates
`output/<run-id>/` containing `accounts.csv`, `report.json`,
`diagnostics.log`, and one `source-NNNN.trace.csv` per input. The directory and
trace paths are printed to stderr. Inputs remain in `tests/fixtures/input`; failed
runs retain `accounts.partial.csv` and a failure report. Use `--output-dir output`
to select another output root. Stdout continues to contain only account CSV data.

The first input exercises exact decimal arithmetic, dispute and resolution,
an original replay, insufficient funds, chargeback and account locking, an
unknown reference, and negative available funds while a deposit is held.
The second exercises normalized decimal replays, resolution without a dispute,
insufficient funds, and four-place decimal addition.

Clients 1–3 belong to the first source, and 4–5 to the second. Original transaction
IDs are disjoint. No lifecycle reference crosses sources, so both files can also
be run individually. A held deposit can make available funds negative under the
engine's existing dispute rules; client 3's expected result is intentional.

## Random samples

The standard-library Python generator accepts a data-row count and optional seed:

```sh
python scripts/generate_payments.py --rows 1000 --seed 42 --output tests/fixtures/input/generated-single.csv
cargo run -- tests/fixtures/input/generated-single.csv
```

It generates deposits, funded withdrawals, and applicable dispute/resolve/
chargeback actions. Every generated event should be applied. Integer amounts
preserve exact four-place decimals. A seed reproduces the sample for the same
arguments and Python version; small samples may not include every event type.
Zero rows produces a valid header-only input. Existing output files are preserved.
Omit `--output` to write CSV to stdout. Python 3.10+ is required.

For a batch, reserve separate client and original transaction ID ranges:

```sh
python scripts/generate_payments.py --rows 1000 --seed 42 --clients 10 --client-start 1 --tx-start 1 --output tests/fixtures/input/generated-a.csv
python scripts/generate_payments.py --rows 1000 --seed 43 --clients 10 --client-start 11 --tx-start 1001 --output tests/fixtures/input/generated-b.csv
cargo run -- tests/fixtures/input/generated-a.csv tests/fixtures/input/generated-b.csv
```

`--clients` sets the client pool size, and `--client-start`/`--tx-start` set its
ID ranges. The generator reserves up to one original transaction ID per data row;
use that upper bound to partition independent samples. Lifecycle rows reuse
IDs only within their own generated sample. The generator keeps at least one
client unlocked to remain able to generate the requested number of valid rows.
