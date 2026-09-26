"""Mutation testing of the Rust engines and decoder.

A test suite that never fails proves little. This script plants one deliberate bug at a time in a
COPY of the sources (the repository is never touched), then runs the three verification layers:

  A  the 76 pre-existing `cargo test` tests   (failing test outside engines::properties)
  B  the 37-case runtime checklist            (`market_sim test engine all`, exit status != 0)
  C  the new property tests                   (failing test inside engines::properties)

A mutant is "killed" when at least one layer fails. Every mutation replaces one exact source
string, and the script refuses to run a mutation whose target text is not found exactly once.

Usage: python analysis/run_mutation_tests.py            (about 20-30 minutes)
Output: analysis/output/verification_mutations.json
"""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
SRC = REPO / "src"
WORK = Path(os.environ.get("MUT_WORK", REPO / "analysis" / "output" / "_mutation_work"))
OUT = Path(__file__).parent / "output"

# (id, description, file, exact old text, new text, occurrence to change: 1-based, or 0 = must be unique)
MUTANTS = [
    ("C1", "CDA: buy crosses only when strictly above the ask (>= becomes >)", "engines/cda.rs",
     "Side::Buy => *taker_px >= *maker_px", "Side::Buy => *taker_px > *maker_px", 0),
    ("C2", "CDA: sell crossing direction flipped", "engines/cda.rs",
     "Side::Sell => *taker_px <= *maker_px", "Side::Sell => *taker_px >= *maker_px", 0),
    ("C3", "CDA: within a price level the LATEST order has priority", "engines/cda.rs",
     "fn arrival_key(o: &Order) -> (u64, u64) {\n    (o.ts, o.oid)", "fn arrival_key(o: &Order) -> (u64, u64) {\n    (u64::MAX - o.ts, o.oid)", 0),
    ("C4", "CDA: trade prints at the taker's limit instead of the maker's", "engines/cda.rs",
     "let execution_price = get_price(&best_ask.kind());", "let execution_price = match order.kind() { OrderKind::Limit { price } => price, OrderKind::Market => get_price(&best_ask.kind()) };", 0),
    ("C5", "CDA: cancel forgets to reduce the bid depth counter", "engines/cda.rs",
     "self.bid_depth = self.bid_depth.saturating_sub(removed.remaining);", "", 0),
    ("C6", "CDA: an unfilled limit sell is not rested", "engines/cda.rs",
     "if order.remaining > 0 && matches!(order.kind(), OrderKind::Limit { .. }) {", "if false && matches!(order.kind(), OrderKind::Limit { .. }) {", 2),
    ("C7", "CDA: a partially filled maker is removed from the book", "engines/cda.rs",
     "let ask_fully_filled = best_ask.remaining == 0;", "let ask_fully_filled = true;", 0),
    ("F1", "FBA: tie-break prefers the LARGER imbalance", "engines/fba.rs",
     "imbalance < best_imbalance", "imbalance > best_imbalance", 0),
    ("F2", "FBA: tie-break by distance to the previous clearing price removed", "engines/fba.rs",
     "diff_new < diff_best || (diff_new == diff_best && price < best_price)", "price < best_price", 0),
    ("F3", "FBA: sells are rationed from the most expensive price", "engines/fba.rs",
     "(Side::Sell, OrderKind::Limit { price }) => (1, price),", "(Side::Sell, OrderKind::Limit { price }) => (1, u128::MAX - price),", 0),
    ("F4", "FBA: within a price the LATEST order is served first", "engines/fba.rs",
     "(aggressiveness, order.ts, order.oid)", "(aggressiveness, u64::MAX - order.ts, order.oid)", 0),
    ("F5", "FBA: supply counts sells strictly below the price only (<= becomes <)", "engines/fba.rs",
     "sell_prices.partition_point(|&p| p <= price)", "sell_prices.partition_point(|&p| p < price)", 0),
    ("F6", "FBA: buy market orders are ignored in the demand schedule", "engines/fba.rs",
     "(Side::Buy, OrderKind::Market) => buy_market_qty += order.remaining,", "(Side::Buy, OrderKind::Market) => buy_market_qty += 0,", 0),
    ("F7", "FBA: the unfilled residual is dropped instead of rolled over", "engines/fba.rs",
     "self.pending_orders = residual_orders;", "self.pending_orders = Vec::new();", 0),
    ("F8", "FBA: previous clearing price updated even when nothing traded", "engines/fba.rs",
     "if traded_quantity > 0 {\n            self.last_clearing_price", "if true {\n            self.last_clearing_price", 0),
    ("D1", "Decoder: quantities truncated instead of rounded half-up", "inputs/binary_format.rs",
     "(scaled + PRICE_SCALE / 2) / PRICE_SCALE", "scaled / PRICE_SCALE", 0),
    ("D2", "Decoder: seven-decimal prices truncated instead of rounded", "inputs/binary_format.rs",
     "(value + divisor / 2) / divisor", "value / divisor", 0),
    ("D3", "Classification: self-trade cancellations (status 12) not treated as cancellations", "types.rs",
     "matches!(self.status_id, 2 | 7 | 10 | 11 | 12 | 13 | 14 | 16)", "matches!(self.status_id, 2 | 7 | 10 | 11 | 13 | 14 | 16)", 0),
    ("D4", "Classification: conditional orders that have not triggered are treated as live", "types.rs",
     "1 => !self.is_trigger,", "1 => true,", 0),
]


def run(cmd, cwd, env, timeout=1800):
    return subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)


def failing_tests(text: str) -> list[str]:
    return sorted(set(re.findall(r"^test (\S+) \.\.\. FAILED", text, flags=re.M)))


def main():
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(WORK / "target"), CARGO_TERM_COLOR="never")
    only = set(sys.argv[1:])
    results = []
    prev = OUT / "verification_mutations.json"
    if only and prev.exists():                           # partial re-run: keep the other results
        results = [r for r in json.loads(prev.read_text()) if r["id"] not in only]
    for mid, desc, rel, old, new, occ in MUTANTS:
        if only and mid not in only:
            continue
        copy = WORK / "src"
        if copy.exists():
            shutil.rmtree(copy)
        shutil.copytree(SRC, copy, ignore=shutil.ignore_patterns("target", "data", "output", ".git"))
        f = copy / rel
        text = f.read_text(encoding="utf8")
        n = text.count(old)
        if (occ == 0 and n != 1) or (occ > 0 and n < occ):
            results.append({"id": mid, "description": desc, "error": f"target text found {n} times"})
            print(mid, "SKIPPED:", n, "matches")
            continue
        if occ == 0:
            text = text.replace(old, new)
        else:                                            # change only the occ-th occurrence
            pos = -1
            for _ in range(occ):
                pos = text.index(old, pos + 1)
            text = text[:pos] + new + text[pos + len(old):]
        f.write_text(text, encoding="utf8")

        t = run(["cargo", "test", "--no-fail-fast"], copy, env)
        out = t.stdout + t.stderr
        compile_error = "error[" in out or "could not compile" in out
        failed = failing_tests(out)
        layer_a = [x for x in failed if "properties::" not in x]
        layer_c = [x for x in failed if "properties::" in x]
        b = run(["cargo", "run", "--quiet", "--", "test", "engine", "all"], copy, env)
        layer_b = b.returncode != 0 and "error[" not in (b.stderr or "")
        killed = bool(layer_a or layer_b or layer_c)
        row = {"id": mid, "description": desc, "compile_error": compile_error, "killed": killed,
               "layer_A_cargo_tests": layer_a, "layer_B_checklist_failed": layer_b, "layer_C_property_tests": layer_c}
        results.append(row)
        print(mid, "KILLED" if killed else "SURVIVED", "| A:", len(layer_a), "B:", layer_b, "C:", len(layer_c), flush=True)
    order = {m[0]: i for i, m in enumerate(MUTANTS)}
    results.sort(key=lambda r: order[r["id"]])
    (OUT / "verification_mutations.json").write_text(json.dumps(results, indent=1))
    shutil.rmtree(WORK, ignore_errors=True)


if __name__ == "__main__":
    main()
