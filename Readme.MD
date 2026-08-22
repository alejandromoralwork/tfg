# Discrete-Time Market Models: CDA vs. Frequent Batch Auctions

Bachelor's thesis project comparing Continuous Double Auctions (CDA) against Frequent
Batch Auctions (FBA), in two connected parts.

**Theoretical part.** The design space of an FBA matching engine, broken into its
architectural blocks (batch interval, information disclosure, order flow segregation,
matching/rationing rules, settlement) and analysed with market microstructure and
auction theory to derive the strategic behaviour each design choice induces.

**Empirical part.** Two matching engines implemented from scratch in Rust — a
continuous engine (CDA) and a single, standard-configuration batch auction engine
(FBA) — fed with identical historical order flow so that the two market institutions
can be compared on a strictly like-for-like basis. The FBA clears each batch at a
uniform price that maximizes executed volume subject to flow conservation and limit
price bounds; volume that cannot be matched at that price simply remains unexecuted
(rolled over or cancelled per time-in-force) — no external liquidity source (e.g. an
AMM) is used to fill it. Both engines trade a single fixed pair, SOL/USD.

The full research design, research questions, metric catalogue, and methodology are
written up in [`docs/expose.tex`](docs/expose.tex).

## Repository layout

```
docs/expose.tex                  Thesis exposé (research questions, methodology, metrics)

src/                              Rust crate `market_sim` — both matching engines, the CLI, and metrics
  Cargo.toml, main.rs             Crate root (Cargo.toml lives here, not a level above — see src/docs/init.md)
  types.rs                        Shared Order/Trade/Side/EngineKind types (single-pair, PRICE_SCALE-fixed-point)
  engines/
    fba.rs                        Batch auction engine: closed-form uniform-price clearing
    cda.rs                        Continuous engine: price-time priority limit order book
  inputs/
    cli.rs                        Interactive command prompt (parses + dispatches every command below)
    simulator.rs                  CSV preview loader; real binary/gzip streaming reader; directory traversal
    binary_format.rs              Decoder for the dataset's packed 54-byte order-status record
    simulate_cmd.rs                `simulate` command: replays real data through both engines, writes output/
    download_cmd.rs                `download` command: fetches BTC/ETH/SOL data from Zenodo via curl + tar
    test_suite.rs                  Built-in engine checklist (`test engine <continuous|batch>`)
  metrics/
    stats.rs                      On-demand metrics for the interactive `metrics`/`orderbook` commands
    timeseries.rs                  Full RQ2.1/2.2/2.3 + engine-performance time-series metric catalogue
  docs/                            Engine design notes, dataset schema copy, build/run walkthrough (see below)
  data/                            Per-coin archives fetched by `download` (git-ignored, created on demand)
  output/                          `simulate`'s time-series CSVs + summary report (git-ignored, created on demand)

data/                              Top-level Hyperliquid L4 dataset tree (mostly git-ignored — see Data below)
  SCHEMA.md, read_data.py          Dataset documentation + the dataset's own reference Python reader
  sample/                          Small, git-tracked previews of the data (a few hundred rows/lines)

download_data.sh                   Bootstrap script: downloads + extracts SOL order/rejected data + mapdir

old/thesis-market-models/          Archived first-draft implementation (multi-crate workspace) — superseded
                                    by src/, kept only for reference; not built or tested
```

## Data

Real order flow can be fetched two ways, feeding two separate locations:

**1. `download_data.sh` (repo root)** — the original bootstrap script, fetches SOL
order-status data **and** its rejected counterpart **and** the `mapdir` lookup tables
into the top-level `data/` tree:

```bash
./download_data.sh                   # downloads + extracts, deletes archives afterwards
./download_data.sh --keep-archives   # keeps the downloaded .tar.xz files too
```

This is what originally populated `data/order_statuses/` and `data/mapdir/`, and is
what `data/sample/` (git-tracked, a few hundred rows) is a preview of. Requires `curl`
and `tar` with `.xz` support (both ship by default on Windows 10 1803+ and virtually
every Linux/macOS install).

**2. The `download` command, from inside the simulator itself** — fetches only the
order-status archive (no rejected orders, no mapdir — the engines never need either;
see `Order::is_new_live_order` in `src/types.rs`) for whichever coin(s) you ask for,
straight into `src/data/order_statuses/<coin>/`, ready for `simulate` to consume
immediately:

```
sim [FBA]> download sol          # or: btc | eth | all
```

Both pull from the same Zenodo record (Albers, Cucuringu, Howison & Shestopaloff,
2026): <https://zenodo.org/records/18184441>. It provides, per order, the full
accept/cancel/fill/reject lifecycle, both counterparties to every trade, post-trade
inventory, and nanosecond-precision timestamps — L4 granularity, which is what makes
it possible to replay real historical order flow order-by-order through a different
matching engine. See `data/SCHEMA.md` for the full field-by-field schema.

The datasets are large (single-coin order-status archives run 6–19 GB compressed) and
are **excluded from git** in both locations (see `.gitignore`).

## Building & running

```bash
cd src
cargo build
cargo run
```

`Cargo.toml` lives directly in `src/` (not a level above), so build/run commands must
be issued from there — see [`src/docs/init.md`](src/docs/init.md) for the full
walkthrough (prerequisites, a first quick session, running the already-built binary
directly). This project's only external dependency is `flate2` (gzip decoding for
`simulate`'s real-data intake) — everything else, including all price/quantity
arithmetic, is pure integer code with zero floating-point pricing.

## Commands

The interactive prompt supports (run `help` inside it for the authoritative, current
list):

| Command | Purpose |
|---|---|
| `engine <continuous\|batch>` | Switch the active engine (CDA / FBA) |
| `add <buy\|sell> <price\|market> <qty> <user>` | Submit a limit or market order to the active engine |
| `batch` | Show the active engine's pending/resting order state |
| `clear` | Force-close the current FBA batch window |
| `load <path> [path...]` | Replay CSV PREVIEW file(s) into the active engine |
| `simulate <path\|btc\|eth\|sol> [interval_secs]` | Replay real `.csv`/`.gz` data (a file, folder, or downloaded coin) through **both** engines, writing time-series metrics to `output/` |
| `download <btc\|eth\|sol\|all>` | Fetch that coin's order-status archive from Zenodo into `data/order_statuses/<coin>/` |
| `log` | Chronological ledger of every executed trade (both engines) |
| `metrics` | Point-in-time metrics for both engines |
| `orderbook` (alias `ob`) | Active engine's order book state + its own metrics, in one view |
| `test engine <continuous\|batch>` | Run the built-in correctness checklist against a fresh instance of that engine |
| `help` / `exit` | |

## Engine internals

For how the FBA clearing price is actually computed and how an order flows through
each module from submission to settlement, see
[`src/docs/ENGINE_DESIGN.md`](src/docs/ENGINE_DESIGN.md).

## Testing

```bash
cd src
cargo test                     # unit tests (parsers, decoders, metric math, ...)
cargo test -- --ignored        # + the real-sample-data integration test
```

Run `cargo test` from `src/`. For a live, end-to-end correctness check of either
matching engine against a fresh instance, use the in-app checklist instead:

```
sim [FBA]> test engine batch
sim [FBA]> test engine continuous
```
