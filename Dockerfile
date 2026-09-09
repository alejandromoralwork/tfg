# syntax=docker/dockerfile:1
#
# market_sim — container for non-interactive runs (scripting, GCP Batch).
#
#   docker build -t market_sim .
#   docker run --rm \
#     -v "$PWD/data:/work/data:ro" -v "$PWD/out:/work/output" \
#     market_sim simulate data/sample/order_statuses/20251201 1
#
# See src/docs/DEPLOY.md for the Google Cloud Batch job spec.

# ---- build ----------------------------------------------------------------
FROM rust:1.83-slim-bookworm AS build
WORKDIR /src

# The crate's manifest lives in src/ and its [[bin]] path is "main.rs"
# (next to the manifest). Copy just the manifest + lockfile first and build
# a stub binary so the dependency compile (flate2, colored) caches on the
# lockfile alone and is skipped whenever only source changes.
COPY src/Cargo.toml src/Cargo.lock ./
RUN echo 'fn main() {}' > main.rs \
 && cargo build --release --locked \
 && rm -f main.rs target/release/market_sim target/release/deps/market_sim-*

COPY src/ ./
RUN cargo build --release --locked

# ---- runtime --------------------------------------------------------------
FROM debian:bookworm-slim
# `simulate` / `scan` shell out to nothing (gzip is decoded in-process).
# ca-certificates is only needed if you also run `download`/`update`; add
# `curl xz-utils tar` to this line for in-container `download`.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/market_sim /usr/local/bin/market_sim

# CWD-relative paths: `simulate sol` reads ./data/order_statuses/sol and
# writes ./output/sol — mount your data bucket at /work/data and your
# output bucket at /work/output.
WORKDIR /work
ENTRYPOINT ["market_sim"]
