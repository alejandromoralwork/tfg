# Testing Reference — how every test is run and what it proves

> See [`README.md`](README.md) for the full documentation index and
> [`ARCHITECTURE.md`](ARCHITECTURE.md) for the whole-project map.

This project has **two independent, non-overlapping verification systems**. Knowing which one
answers your question matters:

| | `cargo test` | `test engine <target>` |
|---|---|---|
| What it is | 80 inline `#[test]` functions, standard Rust unit/differential tests | 37 hand-built checklist cases, compiled into the binary itself |
| Where it lives | `#[cfg(test)] mod tests { ... }` blocks in 11 source files | `src/inputs/test_suite.rs` (production code, not `#[cfg(test)]`) |
| Needs a Rust toolchain? | Yes — `cargo` to compile and run | No — runs inside the already-built `market_sim` binary |
| What it tests | Parsers, decoders, pure math, data-structure invariants, differential (naive-scan) checks | Whole-engine matching *behavior* (CDA/FBA scenarios end to end) and the entire 35-column metrics catalogue, each against a hand-computed value |
| How to run | `cd src && cargo test` | `test engine all` inside the REPL, or `market_sim test engine all` as an argv command |
| Used for | Local dev / CI-shaped correctness of the plumbing | The one thing that also works with **no Rust toolchain** — the Docker image / GCP Batch smoke test runs this |

Neither system is a superset of the other: `cargo test` never runs a synthetic multi-order
matching scenario through `CdaOrderBook`/`FbaOrderBook` end to end (beyond the differential
fuzz tests noted below), and `test engine` never checks the CSV parser, the binary decoder, or
the checkpoint-persistence format. Use both.

---

## 1. Running `cargo test` in the terminal

```
cd src
cargo test                      # every non-ignored test — 76 of the 80 total
cargo test -- --ignored         # only the 4 ignored tests (need real data or real tools)
cargo test <substring>          # filter by test name substring, e.g.:
cargo test kyle_lambda          #   -> runs both kyle_lambda tests in metrics/timeseries.rs
cargo test --release            # same tests, optimized build (rarely needed — these are fast)
```

There is no separate `tests/` integration-test directory — every one of the 80 tests is an
inline `#[cfg(test)] mod tests { ... }` block inside the file it tests. `cargo test` discovers
all of them from one invocation at the crate root (`src/`).

### The 4 `#[ignore]`d tests and their preconditions

These need something `cargo test`'s default run can't assume is present, so they're excluded
unless you explicitly ask for them:

| Test | File | Precondition |
|---|---|---|
| `extract_archive_produces_the_expected_files` | `inputs/download_cmd.rs` | Real `tar` and `xz` executables runnable on the machine (it shells out to them to build and extract a synthetic archive) |
| `scan_reproduces_known_totals_for_the_real_sample_data` | `inputs/scan_cmd.rs` | `data/sample/order_statuses/` present at the fixed relative path `../data/sample/order_statuses` from `src/` |
| `peek_first_ts_matches_the_full_stream_on_the_real_sample` | `inputs/simulator.rs` | `data/sample/order_statuses/20251201/sol_12.data.gz` present at a fixed relative path |
| `streams_the_real_sample_gz_file_correctly` | `inputs/simulator.rs` | Same real sample `.gz` file as above |

All four need the repo's bundled `data/sample/` tree (git-tracked, a few hundred rows — see the
root README's Data section) or system tools already present on most Windows/Linux/macOS
installs. Run them with `cargo test -- --ignored` once you have the sample data / tools
available; they're skipped by default because CI or a fresh clone might not.

---

## 2. `cargo test` catalog — every test, by file

80 tests total across 11 files. Grouped in the order they'd typically be read; ignored tests
are marked **[ignored]**.

<a id="cargo-cda"></a>
### `src/engines/cda.rs` — 6 tests
Module under test: `CdaOrderBook` (the continuous engine).

1. `bid_ask_depth_matches_hand_computed_scenario` — resting buys, a partial cross, a
   non-crossing sell, a cancel, and a cancel-of-nonexistent-oid; `bid_depth()`/`ask_depth()`
   checked at each step against hand values and a naive full-scan reference.
2. `matching_respects_price_then_time_priority_across_and_within_levels` — three resting asks
   (two same price, one worse) swept by a market buy; asserts price-then-time fill order and
   no empty price levels remain.
3. `out_of_order_arrival_at_same_price_still_sorts_by_ts_not_submission_order` — two same-price
   sells submitted out of `ts` order; asserts a later market buy still fills by `ts`, not by
   submission order (exercises the `insert_sorted` binary-search-insert fallback).
4. `cancel_of_an_oid_that_already_fully_filled_is_a_harmless_no_op` — fully fills a resting
   order, then cancels that same (now-gone) oid; asserts `false` returned, not a panic.
5. `fully_filled_or_canceled_price_level_leaves_no_dangling_entry` — asserts a price level is
   actually removed from the book (not left as an empty `VecDeque`) once fully filled or fully
   cancelled.
6. `accessors_match_naive_scan_after_random_submit_cancel_sequence` — seeded-LCG randomized
   500-step fuzz test; every accessor (`bid_depth`, `ask_depth`, `bid_count`, `ask_count`,
   `best_bid`, `best_ask`) checked against an independent naive full-scan after every step.

<a id="cargo-fba"></a>
### `src/engines/fba.rs` — 3 tests
Module under test: `demand_supply_evaluators` (the batch engine's demand/supply curve builder).

1. `demand_supply_evaluators_boundary_prices_match_filter_semantics` — mixed buy/sell
   limit/market batch straddling a target price; asserts the prefix-sum/binary-search
   evaluators match a naive linear-scan reference exactly at the boundary.
2. `demand_supply_evaluators_handle_empty_and_all_market_batches` — an empty batch returns zero
   at any price; an all-market batch returns the same total regardless of the queried price
   (checked at `0` and `u128::MAX`).
3. `demand_supply_evaluators_match_naive_scan_across_random_batches` — seeded-LCG differential
   test, 200 random batches × 20 random query prices each, against the naive O(n) reference.

### `src/inputs/binary_format.rs` — 11 tests
Module under test: the 54-byte packed order-status decoder.

1. `decodes_the_schema_worked_example` — `SCHEMA.md`'s own `$96,543.21` worked example decodes
   to the expected `PRICE_SCALE` value.
2. `decode_price_handles_every_decimal_count` — 0/6/7 decimal-place encodings, including the
   round-half-up needed when the source has one more digit than `PRICE_SCALE` holds.
3. `decode_signed_price_respects_the_sign_bit` — same magnitude, sign bit set vs. unset.
4. `decode_qty_rounds_to_whole_units` — 39.35→39 (down), 39.5→40 (half-up).
5. `parse_record_decodes_a_realistic_record` — a hand-assembled 54-byte open-buy-limit record;
   every decoded field checked, plus `is_new_live_order()` is true.
6. `parse_record_matches_real_sample_data_exactly` — the literal first 54 bytes of the real
   `data/sample/order_statuses/20251201/sol_12.data.gz` (hardcoded inline, not read from disk —
   so this test is *not* ignored), cross-checked field-by-field against the known-correct CSV
   PREVIEW row for the same record.
7. `parse_record_skips_zero_size_after_rounding` — a record whose `origSz` rounds to zero →
   `None`.
8. `looks_like_order_status_record_accepts_a_plausible_record` — a well-formed record passes.
9. `looks_like_order_status_record_rejects_all_zero_bytes` — all-zero buffer (epoch-1970
   timestamp) rejected.
10. `looks_like_order_status_record_rejects_ts_outside_the_plausible_window` — timestamps
    around 2001 and 2065 rejected (outside the 2020–2035 plausibility window).
11. `looks_like_order_status_record_rejects_out_of_range_lookup_ids` — `status_id`/
    `order_type_id`/`tif_id` one past their documented max are each rejected.

<a id="cargo-cli"></a>
### `src/inputs/cli.rs` — 15 tests
Module under test: `CliCommand::parse` (the REPL's line parser).

1. `parses_add_limit_order` — `add buy 127 5 Alice` / `add sell 130 3 Bob`, price scaled to
   `PRICE_SCALE`.
2. `parses_add_with_decimal_price_and_quantity` — `127.06 5.5` → correct scaling + round-half-up
   quantity (regression test for a real bug).
3. `parses_add_market_order` — `add buy market 15 999` and a case-insensitive variant → `price:
   None`.
4. `rejects_malformed_add_commands` — too few args / invalid side / non-numeric price / non-
   numeric qty, each → `None`.
5. `parses_engine_switch_and_its_aliases` — `continuous`/`cda` and `batch`/`fba` aliases; bad/
   missing mode → `None`.
6. `parses_simple_no_argument_commands` — `batch`, `clear`, `log`, `metrics`/`stats`,
   `orderbook`/`ob`, `help`, `exit`/`quit` and their aliases.
7. `parses_load_with_one_or_more_paths` — one or many paths accepted; no path → `None`.
8. `parses_simulate_with_and_without_an_interval_override` — with/without the interval arg;
   missing path or non-numeric interval → `None`.
9. `parses_scan_command` — path required, rejects a path-less call.
10. `parses_download_command_and_rejects_unknown_coins` — `btc`/`eth`/`sol`/`all`
    case-insensitively; unknown/missing coin → `None`.
11. `parses_extract_command_and_rejects_unknown_coins` — same shape as `download`.
12. `parses_update_command_with_and_without_a_branch` — with/without an explicit branch.
13. `parses_test_engine_command_and_its_aliases` — `continuous|cda|batch|fba|metrics|all`
    aliases; malformed forms (`test`, `test something continuous`) → `None`.
14. `rejects_unknown_commands` — unrecognized word, empty string, whitespace-only string → all
    `None`.
15. `strips_leading_bom_before_matching_the_command_name` — a leading `\u{feff}` (Windows-piped
    stdin BOM) doesn't break parsing.

### `src/inputs/download_cmd.rs` — 6 tests (1 ignored)
Module under test: `Coin`, `dest_already_populated`, `dir_size`, `extract_archive` (`download`/
`extract`).

1. `coin_labels_match_the_archive_filename_prefix` — `label()` → `"btc"`/`"eth"`/`"sol"`.
2. `builds_the_expected_zenodo_url_per_coin` — exact expected download URL per coin.
3. `all_returns_every_coin_exactly_once` — `Coin::all()` has exactly the three, no repeats.
4. `dest_already_populated_detects_nested_gz_files` — empty temp dir → unpopulated; a nested
   `.gz` file added → populated.
5. `dir_size_sums_nested_files_recursively` — a temp tree with files at two depths; sum is
   correct across depths.
6. **[ignored]** `extract_archive_produces_the_expected_files` — builds a real `.tar.xz` via
   real `tar`/`xz` subprocesses, extracts it through the production function, asserts the
   archive is deleted post-extraction and file contents/sizes survive, plus
   `xz_uncompressed_size` parses real `xz -l --robot` output correctly.

### `src/inputs/progress.rs` — 3 tests
Module under test: `human_bytes`, `format_duration`, `eta_suffix`.

1. `human_bytes_picks_the_largest_unit_that_stays_above_one` — 0 / 512 / 1536 bytes and a real
   6.16 GiB archive size format to the expected unit+precision.
2. `format_duration_drops_lower_units_once_a_larger_one_is_present` — 0/45/69/3661/90000
   seconds format to the expected coarsened strings (e.g. `"1m 09s"`, `"1d 1h"`).
3. `eta_suffix_withholds_the_estimate_until_theres_enough_signal` — no ETA when nothing's done,
   already complete, or too little elapsed time to trust a rate; a correct `"ETA ~10s"` once
   there's enough signal.

### `src/inputs/replay_checkpoint.rs` — 4 tests
Module under test: `slugify`, `Checkpoint` render/parse round-trip, `truncate_data_rows`.

1. `slugify_takes_the_last_component_and_sanitizes` — various paths sanitize to their last
   component; empty string → `"run"`.
2. `checkpoint_round_trips_through_text` — a fully-populated `Checkpoint` (including a folded
   `IntervalMetrics` summary) survives a render→parse round trip exactly.
3. `parse_rejects_a_foreign_version` — an unrecognized `version` number is rejected.
4. `truncate_trims_extra_rows_and_leaves_matching_files_alone` — a real temp CSV with 4 data
   rows truncates to 2 correctly; truncating again at the same length is a no-op.

### `src/inputs/scan_cmd.rs` — 2 tests (1 ignored)
Module under test: `classify` (the `scan` command's record bucketing).

1. `classify_matches_what_the_engines_themselves_would_do` — orders across every
   `status_id`/`is_trigger`/`triggered` combination bucket exactly as `is_new_live_order`/
   `is_cancellation` would gate them.
2. **[ignored]** `scan_reproduces_known_totals_for_the_real_sample_data` — streams the real
   sample directory and asserts the exact known counts from a historical real run (3,630,216
   seen, 54,179 skipped, 2 files processed), plus that the three buckets are mutually
   exclusive and exhaustive.

### `src/inputs/simulate_cmd.rs` — 3 tests
Module under test: `civil_from_days`, `run_timestamp`, `flush_hi` (date math + streaming-flush
watermark).

1. `civil_from_days_round_trips_known_dates` — recovers 2025-12-01, 2025-01-01, and the Unix
   epoch from their day-counts.
2. `run_timestamp_has_the_expected_shape` — exactly 15 chars, underscore at index 8, digits
   elsewhere (`YYYYMMDD_HHMMSS`).
3. `flush_hi_releases_up_to_the_next_files_start_minus_the_markout_guard` — five scenarios (no
   anchor; next-file-start below anchor; next-file-start as the binding cap; this file's own
   tail as the binding cap; no next file peeked) each check the correct grid-aligned watermark.

### `src/inputs/simulator.rs` — 10 tests (2 ignored)
Module under test: `collect_input_files`, `parse_fixed_point`, `round_to_unit`,
`parse_dataset_ts`, `parse_row`, `peek_first_ts`, `stream_records`.

1. `collect_input_files_walks_a_date_folder_tree_in_order` — a temp tree mimicking
   `data/order_statuses/`, files created out of order, plus a stray `README.md`; asserts
   chronological sort, accepted-before-rejected within an hour, unsupported extension excluded.
2. `collect_input_files_accepts_a_single_file_path_too` — a single file path returns a
   one-element vector.
3. `parses_fixed_point_prices` — `"126.67"`, `"127"`, `"0.000001"` scale correctly.
4. `rounds_quantities_to_whole_units` — `"39.35"`→39, `"39.5"`→40, `"5175.0"`→5175.
5. `parses_dataset_timestamps` — two known date/time strings convert to the correct ns epoch.
6. `parses_a_real_preview_row_end_to_end` — one full real-shaped CSV row → correct oid,
   user_id, status_id, side, price, remaining, `is_new_live_order()`.
7. `peek_first_ts_reads_only_the_first_row` — a temp CSV with a header + two rows; only the
   first row's timestamp is returned.
8. `peek_first_ts_returns_none_for_a_wrong_schema_file` — too few columns → `None`.
9. **[ignored]** `peek_first_ts_matches_the_full_stream_on_the_real_sample` — on the real
   sample `.gz`, `peek_first_ts` agrees with the full stream's first record.
10. **[ignored]** `streams_the_real_sample_gz_file_correctly` — end-to-end gzip+binary-decode
    integration: bytes-read is sane, `records_seen == count + records_skipped`, at least six
    figures of records produced, first decoded order matches the known first PREVIEW CSV row.

### `src/metrics/timeseries.rs` — 17 tests
Module under test: `MetricsRecorder` and the metric formulas — see [`METRICS.md`](METRICS.md)
for what each formula computes; this list is about what each test *proves*.

1. `boundary_concentration_matches_hand_computed_value` — six messages + one batch clear vs. a
   hand-computed fraction.
2. `boundary_concentration_respects_inclusive_window_boundaries` — messages exactly at, just
   before, and just after each edge; exact inclusive/exclusive semantics.
3. `boundary_concentration_is_none_for_a_zero_width_batch` — identical open/close ts → `None`.
4. `boundary_concentration_matches_naive_scan_across_random_batches` — seeded-LCG differential,
   200 trials, against a naive nested-loop rescan.
5. `price_at_or_after_ties_prefer_book_snapshot_over_batch_at_same_ts` — a book snapshot and a
   batch clear at the identical ts; the snapshot wins the tie.
6. `price_at_or_after_returns_none_past_the_last_entry` — a markout horizon past the only
   available entry → `None`.
7. `streaming_emit_in_pieces_matches_one_shot_finish` — a 140s randomized CDA stream through
   two recorders (one flushed once at the end, one incrementally); byte-identical CSV rows.
8. `emit_fills_gaps_with_empty_rows_for_a_contiguous_grid` — two snapshots 10 buckets apart;
   all 11 buckets in between are present (empty ones with no spread).
9. `events_for_an_already_emitted_bucket_are_counted_not_stored` — a late event targeting an
   already-flushed bucket is tallied in `late_events_dropped()`, doesn't perturb later output.
10. `kyle_lambda_cda_recovers_a_known_slope` — engineered observations with a true slope of
    2.0; recovered to within `1e-9`; a net-zero sweep is inert.
11. `kyle_lambda_cda_is_none_when_the_markout_mid_is_missing` — no +5s snapshot → `None`.
12. `kyle_lambda_fba_from_net_order_flow` — five batches engineered for a true slope of 125.0;
    recovered to within `1e-6`; a no-clearing-price batch is inert.
13. `kyle_lambda_fba_carries_prev_clearing_across_a_flush` — bucket 1's lambda correctly uses
    the clearing price carried over from a bucket-0-only flush.
14. `streaming_fba_batches_emit_in_pieces_matches_one_shot` — same streaming-equivalence
    guarantee as #7, for FBA, specifically exercising the `prev_clearing`/`prev_close` carries.
15. `realized_volatility_is_root_sum_of_squared_returns` — three mids → root-sum-of-squares,
    not stddev; a single return in a bucket reports the absolute value.
16. `amihud_illiquidity_divides_the_return_by_dollar_volume` — first bucket has no value (no
    prior close); second bucket matches the hand-computed `|return|/dollar_volume·1e6`.
17. `intra_interval_price_dispersion_is_relative_in_bps` — two trades at 100 and 102; matches
    the hand-computed relative-stddev-in-bps value.

### Totals

| File | Tests | Ignored |
|---|---|---|
| `engines/cda.rs` | 6 | 0 |
| `engines/fba.rs` | 3 | 0 |
| `inputs/binary_format.rs` | 11 | 0 |
| `inputs/cli.rs` | 15 | 0 |
| `inputs/download_cmd.rs` | 6 | 1 |
| `inputs/progress.rs` | 3 | 0 |
| `inputs/replay_checkpoint.rs` | 4 | 0 |
| `inputs/scan_cmd.rs` | 2 | 1 |
| `inputs/simulate_cmd.rs` | 3 | 0 |
| `inputs/simulator.rs` | 10 | 2 |
| `metrics/timeseries.rs` | 17 | 0 |
| **Total** | **80** | **4** |

(`types.rs`, `engines/mod.rs`, `metrics/stats.rs`, `metrics/mod.rs`, and `inputs/update_cmd.rs`
have no `#[cfg(test)]` block at all — `update_cmd.rs` in particular is the one production file
in `inputs/` with zero test coverage of any kind, cargo or runtime.)

---

<a id="runtime-checklist"></a>
## 3. The runtime `test engine` checklist

`src/inputs/test_suite.rs` — **not** `#[cfg(test)]`, compiled into every build of the binary
itself, so it runs with no Rust toolchain at all. This is what makes a checklist runnable
inside the Docker image / a GCP Batch task.

### 3.1 Running it

Interactively:

```
sim [FBA]> test engine continuous     # or: cda   (alias)
sim [FBA]> test engine batch          # or: fba   (alias)
sim [FBA]> test engine metrics
sim [FBA]> test engine all            # all three checklists in sequence
```

As a non-interactive argv command (works on the built binary with no REPL, and is what
`DEPLOY.md`'s Docker smoke test uses):

```
market_sim test engine all
echo $?     # 0 if every case in every requested checklist passed, 1 if any failed
```

`market_sim test engine <target>` accepts the same targets/aliases as the interactive form.
Malformed invocations (missing `engine`, an unknown target, or `test` with no further
arguments) exit **2** (a request error, not a test failure) — see `REFERENCE.md`'s `cli.rs`
section for the full `run_once` exit-code table (0/1/2).

<a id="output-format"></a>
### 3.2 Output format

```
==========================================================================
                       {ENGINE LABEL} TEST CHECKLIST
==========================================================================
  [PASS] <case_name>
  [FAIL] <case_name>
      -> <detail>
--------------------------------------------------------------------------
  RESULT: {passed}/{total} passed — {ENGINE LABEL} OK          (all passed)
  RESULT: {passed}/{total} passed — {N} case(s) FAILING        (otherwise)
==========================================================================
```
`[PASS]` prints green, `[FAIL]`/its detail line print red. `print_checklist` returns whether
every case passed; `test engine all` ANDs that result across all three checklists it runs
(CDA, then FBA, then metrics) to decide the final 0/1.

<a id="checklist-cases"></a>
### 3.3 The 37 cases, condensed (full scenario numbers are in the source's own comments)

`inputs/test_suite.rs` builds every case from tiny deterministic helpers (`limit`, `market`,
`non_live`, `cancel_event`, `filled_event` for the engine checklists; `tmsg`/`ttrade`/
`tbook`/`tbatch` family for the metrics checklist) — explicit ids/timestamps/prices, never
wall-clock, so every run is bit-for-bit reproducible.

**`run_cda_tests()` — 15 cases** (fresh `CdaOrderBook` per case): resting order with no cross;
exact-quantity cross; partial fill of a larger resting order; a multi-fill sweep walking two
price levels; price beats time priority; time (FIFO) priority at the same price; a market order
crosses at the resting maker's own price; a market order with no liquidity neither trades nor
rests; a market order's partial-liquidity fill correctly yields `fill_rate = 0.8` not `1.0`
(regression test); a non-live status row is filtered out entirely; a sell only crosses a resting
bid at-or-below the bid price, never above it (regression test for a real matching-direction
bug); a cancellation removes the matching resting order; cancelling an unknown oid is harmless;
a `filled`-status row never touches a resting order (fills aren't replayed); and one full
hand-computed metrics scenario (specific `quoted_spread_bps`, `book_imbalance`, `fill_rate`,
etc. against numbers worked out by hand in the source).

**`run_fba_tests()` — 14 cases** (fresh `FbaOrderBook` per case): clearing an empty batch
returns `None`; a simple full match; price-time-priority rationing when one side outweighs the
other (the `ENGINE_DESIGN.md` §1.3 worked example, as a running test); a market order queues on
submit and only executes at `clear()`, at the volume-maximizing uniform price; a tie with no
price history picks the lower candidate price; an all-market batch with no price history
preserves every order rather than losing them (regression test for a real bug); an all-market
batch **with** stale clearing-price history still rolls over rather than pricing off that
history; a non-live row is filtered; a cancellation removes the matching pending order;
cancelling an unknown oid is harmless; a `filled`-status row never touches a pending order;
residual (partially-filled) volume rolls into the next batch; a full hand-computed metrics
scenario after a partial clear (`unexecuted_residual_share`, `fill_rate`, `depth_at_best`,
`quoted_spread_bps == None` because the sell side fully cleared); and a tie **with** price
history correctly flips the winner to the price closer to `last_clearing_price`.

**`run_timeseries_metric_tests()` — 8 cases**, driving `metrics::timeseries::MetricsRecorder`
directly with synthetic events (a different code path from the engine-getter tests above):
CDA liquidity/book metrics (`quoted_spread_bps`, `depth_at_best`, `book_imbalance`,
`total_book_depth`, all three depth-within-bps bands, `avg_clearing_latency_micros`,
`throughput_orders_per_sec`, `fill_rate`) against one fully hand-worked snapshot pair; CDA
effective/realized/impact spreads across 1s/5s/30s horizons plus `executed_volume`,
`executed_notional`, `vwap`, `trade_count`, `trader_surplus`, `intra_interval_price_dispersion`,
`order_to_trade_ratio`; CDA `realized_volatility` (root-sum-of-squares, not stddev) and
`amihud_illiquidity` (first bucket `None`, second bucket a specific worked value) across two
buckets; CDA execution/allocation metrics (`fill_rate`, `avg_time_to_execution_secs`,
`order_size_inflation`, `order_to_trade_ratio`) with orders that partially and never fill; CDA
`kyle_lambda` recovering an engineered slope of exactly 2.0 from three sweep observations,
including one inert net-zero sweep; the FBA counterpart of the first case
(`boundary_concentration` included, `book_imbalance`/`total_book_depth` correctly absent);
the FBA counterpart of the second case (unsigned effective/realized/impact formulas, since FBA
never sets `aggressor_side`); and FBA `kyle_lambda` recovering an engineered slope of exactly
125.0 across five batches, including one unpriced (inert) batch that must not disturb the carry.

For every hand-computed number in each of the 37 cases, see the source comments in
`inputs/test_suite.rs` directly, or [`METRICS.md`](METRICS.md) for the general formula each
metrics-checklist case is instantiating.

---

## 4. Which to use when

- **Changing a parser, decoder, or piece of pure math** (`simulator.rs`, `binary_format.rs`,
  `replay_checkpoint.rs`'s render/parse, the metric formulas in `timeseries.rs`) — run
  `cargo test`. It's fast, deterministic, and the differential (naive-scan) tests will catch a
  regression in `CdaOrderBook`/`FbaOrderBook`'s accessor/evaluator internals immediately.
- **Changing matching behavior** (how `submit`/`clear` decide what trades, at what price, in
  what order) or **adding/changing a metrics-catalogue column** — run `test engine all`. It
  exercises full scenarios end to end against hand-computed expectations, which the `cargo
  test` differential tests (which check internal consistency, not a specific "correct" outcome)
  don't do.
- **No Rust toolchain available** (inside the built Docker image, a GCP Batch task, or a
  machine without `cargo`) — only `test engine all` is available; see
  [`DEPLOY.md`](DEPLOY.md#3-build-the-container-image-and-push-it-to-artifact-registry) for the
  `docker run --rm <image> test engine all` smoke test run before every deploy.
- **Before a release/deploy** — run both: `cargo test -- --ignored` (needs `data/sample/` and
  `tar`/`xz`) for the fullest local coverage, then `market_sim test engine all` against the
  actual built release binary as the final gate.
