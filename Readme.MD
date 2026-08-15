# Discrete-Time Market Models: CDA vs. Frequent Batch Auctions

Master's thesis project comparing Continuous Double Auctions (CDA) against Frequent
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
AMM) is used to fill it.

The full research design, research questions, metric catalogue, and methodology are
written up in [`docs/expose.tex`](docs/expose.tex).

## Repository layout

```
docs/expose.tex              Thesis exposé (research questions, methodology, metrics)

thesis-market-models/        Rust workspace: the two matching engines + CLI simulator
  engines/src/cda/           Continuous engine (price-time priority limit order book)
  engines/src/fba/           Batch auction engine (uniform-price clearing)
  engines/src/common/        Shared order/trade/order-book types and the matching-engine trait
  simulation/                Interactive CLI to drive either engine and inspect state

data/                        Hyperliquid L4 order book dataset (not tracked in git — see below)
  README.md, SCHEMA.md       Dataset documentation
  read_data.py                Reference Python reader (decodes the binary order-status format)
  download_data.sh            Downloads + extracts the dataset from Zenodo
  sample/                     Small, git-tracked previews of the data (a few hundred rows/lines)
```

## Data

The empirical comparison is fed by the **Hyperliquid Level 4 order book dataset**
(Albers, Cucuringu, Howison & Shestopaloff, 2026), published on Zenodo:
https://zenodo.org/records/18184441

It provides, per order: the full accept/cancel/fill/reject lifecycle, both
counterparties to every trade, post-trade inventory, and nanosecond-precision
timestamps — L4 granularity, which is what makes it possible to replay real historical
order flow order-by-order through a different matching engine.

The dataset (tens of GB) is **excluded from git** (see `.gitignore`). To fetch it:

```bash
cd data
./download_data.sh              # downloads + extracts, deletes archives afterwards
./download_data.sh --keep-archives   # keeps the downloaded .tar.xz files too
```

This populates `data/order_statuses/` and `data/mapdir/`. See `data/README.md` for
the full file layout and `data/SCHEMA.md` for the binary/JSON schema of every field.

## Running the simulator

```bash
cd thesis-market-models
cargo run -p simulation
```

An interactive CLI that lets you switch between the CDA and FBA engines, submit
orders, trigger a batch clearing, and inspect the resulting book/trade state. Run
`cargo test` from `thesis-market-models/` to run the engine unit tests.
