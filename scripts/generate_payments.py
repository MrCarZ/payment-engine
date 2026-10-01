#!/usr/bin/env python3
"""Generate syntactically valid payments whose state transitions all apply.

Uses integer units of 0.0001 and the engine's dispute/locking rules. Python 3.10+
and the standard library are sufficient. The requested row count excludes headers.
"""

import argparse
import csv
import sys
from dataclasses import dataclass
from pathlib import Path
from random import Random
from typing import Iterator

SCALE = 10_000
MAX_CLIENT_ID = 65_535
MAX_TRANSACTION_ID = 4_294_967_295
MAX_AMOUNT_UNITS = 1_000 * SCALE


@dataclass
class Account:
    available: int = 0
    held: int = 0
    locked: bool = False


@dataclass
class Deposit:
    client: int
    tx: int
    amount: int
    pool_index: int = 0
    client_index: int = 0


def remove_from_pool(pool: list[Deposit], index: int) -> Deposit:
    deposit = pool[index]
    last = pool.pop()
    if index < len(pool):
        pool[index] = last
        last.pool_index = index
    return deposit


def remove_from_client(pool: list[Deposit], index: int) -> None:
    last = pool.pop()
    if index < len(pool):
        pool[index] = last
        last.client_index = index


def format_amount(units: int) -> str:
    whole, fraction = divmod(units, SCALE)
    return f"{whole}.{fraction:04d}"


def generate_rows(
    rows: int, clients: int, client_start: int, tx_start: int, seed: int | None
) -> Iterator[tuple[str, int, int, str]]:
    rng = Random(seed)
    accounts = {client: Account() for client in range(client_start, client_start + clients)}
    posted: list[Deposit] = []
    disputed: list[Deposit] = []
    disputed_by_client: dict[int, list[Deposit]] = {client: [] for client in accounts}
    next_tx = tx_start

    for _ in range(rows):
        unlocked = [client for client, account in accounts.items() if not account.locked]
        funded = [client for client in unlocked if accounts[client].available > 0]
        # Never lock the last unlocked client, ensuring every requested row can
        # be generated even after chargebacks. Existing lifecycle actions remain
        # possible for deposits belonging to locked clients.
        chargeback_clients = [
            client for client, deposits in disputed_by_client.items()
            if deposits and (accounts[client].locked or len(unlocked) > 1)
        ]
        actions = ["deposit"]
        weights = [50]
        for action, possible, weight in [
            ("withdrawal", bool(funded), 25),
            ("dispute", bool(posted), 15),
            ("resolve", bool(disputed), 7),
            ("chargeback", bool(chargeback_clients), 3),
        ]:
            if possible:
                actions.append(action)
                weights.append(weight)
        action = rng.choices(actions, weights=weights, k=1)[0]

        if action in ("deposit", "withdrawal"):
            client = rng.choice(unlocked if action == "deposit" else funded)
            account = accounts[client]
            maximum = MAX_AMOUNT_UNITS if action == "deposit" else min(MAX_AMOUNT_UNITS, account.available)
            amount = rng.randint(1, maximum)
            tx = next_tx
            next_tx += 1
            if action == "deposit":
                account.available += amount
                posted.append(Deposit(client, tx, amount, pool_index=len(posted)))
            else:
                account.available -= amount
            yield action, client, tx, format_amount(amount)
            continue

        pool = posted if action == "dispute" else disputed
        if action == "chargeback":
            client = rng.choice(chargeback_clients)
            index = rng.choice(disputed_by_client[client]).pool_index
        else:
            index = rng.randrange(len(pool))
        # Swap removal keeps arbitrary transaction selection inexpensive.
        deposit = remove_from_pool(pool, index)
        account = accounts[deposit.client]
        client_pool = disputed_by_client[deposit.client]
        if action != "dispute":
            remove_from_client(client_pool, deposit.client_index)
        if action == "dispute":
            account.available -= deposit.amount
            account.held += deposit.amount
            deposit.pool_index = len(disputed)
            deposit.client_index = len(client_pool)
            disputed.append(deposit)
            client_pool.append(deposit)
        elif action == "resolve":
            account.available += deposit.amount
            account.held -= deposit.amount
            deposit.pool_index = len(posted)
            posted.append(deposit)
        else:
            account.held -= deposit.amount
            account.locked = True
        yield action, deposit.client, deposit.tx, ""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rows", type=int, required=True, help="number of data rows (excluding the header)")
    parser.add_argument("--clients", type=int, default=10, help="client pool size (default: 10)")
    parser.add_argument("--client-start", type=int, default=1, help="first client ID (default: 1)")
    parser.add_argument("--tx-start", type=int, default=1, help="first original transaction ID (default: 1)")
    parser.add_argument("--seed", type=int, help="seed for reproducible samples")
    parser.add_argument("--output", type=Path, help="create a CSV file; defaults to stdout; existing files are preserved")
    args = parser.parse_args()
    if not 0 <= args.rows <= MAX_TRANSACTION_ID + 1:
        parser.error("--rows must be between 0 and 4294967296")
    if not 1 <= args.clients <= MAX_CLIENT_ID + 1:
        parser.error("--clients must be between 1 and 65536")
    if not 0 <= args.client_start <= MAX_CLIENT_ID:
        parser.error("--client-start must be between 0 and 65535")
    if args.client_start + args.clients - 1 > MAX_CLIENT_ID:
        parser.error("the client pool exceeds the u16 client ID range")
    if not 0 <= args.tx_start <= MAX_TRANSACTION_ID:
        parser.error("--tx-start must be between 0 and 4294967295")
    # Reserve enough IDs even if every generated row is an original. References
    # consume no new IDs, so this conservative bound prevents every overflow.
    if args.rows and args.tx_start + args.rows - 1 > MAX_TRANSACTION_ID:
        parser.error("the requested rows exceed the remaining u32 transaction ID range")
    return args


def write_sample(output, args: argparse.Namespace) -> None:
    writer = csv.writer(output, lineterminator="\n")
    writer.writerow(("type", "client", "tx", "amount"))
    writer.writerows(generate_rows(args.rows, args.clients, args.client_start, args.tx_start, args.seed))
    output.flush()


def main() -> int:
    args = parse_args()
    try:
        if args.output is None:
            write_sample(sys.stdout, args)
        else:
            with args.output.open("x", encoding="utf-8", newline="") as output:
                write_sample(output, args)
    except OSError as error:
        print(f"Cannot write sample: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
