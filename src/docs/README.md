# `market_sim` documentation index

This folder documents the Rust crate `market_sim` — the CDA/FBA matching-engine simulator in
`src/`. For the project's overall pitch, data instructions, and command list, see the
[repo-root README](../../Readme.MD).

**Start here** if you're new to the codebase: [`ARCHITECTURE.md`](ARCHITECTURE.md) — the
whole-project map. Everything else is a deeper dive on one piece of it, cross-linked from
there so nothing is duplicated between files.

| Doc | Read this for |
|---|---|
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | The whole-project map: module boundaries, the order lifecycle from raw record to matched trade, the `simulate` streaming pipeline end to end, and a quick index of the metric catalogue. Start here. |
| [`ENGINE_DESIGN.md`](ENGINE_DESIGN.md) | How the FBA uniform-clearing-price algorithm actually picks a price (candidate prices, three-tier selection, rationing) with a worked example; the module-interaction walkthrough from `add`/`load` to settlement. |
| [`METRICS.md`](METRICS.md) | **How every metric is calculated.** Formula-by-formula, accumulator-by-accumulator reference for all 35 `simulate` time-series CSV columns, including `None`-vs-`0.0` edge cases and where CDA and FBA compute a column differently. |
| [`TESTING.md`](TESTING.md) | **How every test is done.** The full `cargo test` catalog (80 tests across 11 files, with exact terminal commands) and the runtime `test engine` checklist (37 cases, runnable with no Rust toolchain) — what each proves, and which to reach for when. |
| [`REFERENCE.md`](REFERENCE.md) | **The file-by-file reference.** Every public struct, enum, and function in the crate, one section per source file, with exact signatures and precise algorithm descriptions. |
| [`SCHEMA.md`](SCHEMA.md) | Field-by-field schema of the 54-byte binary order-status record this crate decodes, the CSV PREVIEW format, and the `mapdir` lookup tables. |
| [`Cancellations.md`](Cancellations.md) | Design note: why the dataset's `accepted`/`_rejected` file split is *not* "live vs rejected", how cancellations are replayed (and fills deliberately are not), and the unenforced-TIF limitation. |
| [`init.md`](init.md) | Toolchain prerequisites and how to build/run, step by step. |
| [`DEPLOY.md`](DEPLOY.md) | Running `simulate` non-interactively: the Docker image and a full Google Cloud Batch job spec, with checkpoint-backed retry. |
| [`DATASET.md`](DATASET.md) | The upstream Hyperliquid dataset itself (archive sizes, its own Python reader, citation, license) — not `market_sim`'s own code. |

## Answering common questions

- **"How do I run the tests?"** → [`TESTING.md`](TESTING.md) §1–2 for `cargo test` in the
  terminal, §3 for the runtime `test engine` checklist (including as a non-interactive argv
  command with no Rust toolchain).
- **"How is metric X calculated?"** → [`METRICS.md`](METRICS.md), one subsection per column.
- **"How does an order get from the dataset to a trade?"** →
  [`ARCHITECTURE.md` §3](ARCHITECTURE.md#order-lifecycle), then
  [`ENGINE_DESIGN.md`](ENGINE_DESIGN.md) for the FBA clearing-price algorithm specifically.
- **"What does function/struct X do?"** → [`REFERENCE.md`](REFERENCE.md), organized by file.
- **"How do I build/run/deploy this?"** → [`init.md`](init.md) locally, [`DEPLOY.md`](DEPLOY.md)
  for Docker/Google Cloud Batch.
