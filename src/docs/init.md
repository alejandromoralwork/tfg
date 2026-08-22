# Building and running `market_sim`

## Prerequisites

- Rust toolchain (rustup + cargo). Check with:
  ```
  cargo --version
  rustc --version
  ```
- On Windows, the default toolchain (MSVC) also needs the Visual Studio
  Build Tools' "Desktop development with C++" workload installed for
  linking. If `cargo build` fails with a missing `link.exe` error, that's
  what's missing.

## Compile

`Cargo.toml` lives in `c:\Users\pc\other\src\` — build from there (or any
subfolder beneath it; cargo searches upward for `Cargo.toml` automatically):

```
cd c:\Users\pc\other\src
cargo build
```

Compiler warnings about unused fields/functions are expected and don't
block the build — the last line should read
`Finished ... target(s) in ...s` with no errors.

## Run

```
cargo run
```

This builds (if needed) and launches the interactive prompt. Or run the
already-built binary directly:

```
.\target\debug\market_sim.exe
```

## Quick usage

```
sim [FBA]> add buy 127 5 Alice
sim [FBA]> add sell 127 5 Bob
sim [FBA]> clear
sim [FBA]> metrics
sim [FBA]> engine continuous
sim [CDA]> load ../data/sample/order_statuses_accepted_PREVIEW.csv
sim [CDA]> metrics
sim [FBA]> exit
```

`help` inside the prompt lists every command.
