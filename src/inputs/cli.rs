//! Interactive command-prompt loop for driving both engines and testing
//! them by hand or by replaying `data/sample`. Ported from the old
//! `simulation` crate's `commands.rs` + `main.rs` + `display.rs`, folded
//! into one file here since there's no need for three anymore.

use std::io::{self, Write};

use colored::Colorize;

use crate::engines::cda::CdaOrderBook;
use crate::engines::fba::FbaOrderBook;
use crate::inputs::download_cmd::{self, Coin, DownloadTarget};
use crate::inputs::simulate_cmd;
use crate::inputs::simulator;
use crate::inputs::test_suite;
use crate::metrics::stats;
use crate::types::{EngineKind, Order, Side, Trade, PRICE_SCALE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineMode {
    Continuous,
    Batch,
}

#[derive(Debug, PartialEq)]
enum CliCommand {
    // `price: None` means a market order (fills at the resting/eligible
    // book's own price rather than a limit the caller sets). When present,
    // it's already PRICE_SCALE-scaled (see `simulator::parse_fixed_point`)
    // — ready to hand straight to `Order::limit`, no further scaling.
    Add { side: Side, price: Option<u128>, qty: u128, user: String },
    Engine(EngineMode),
    Batch,
    Clear,
    Log,
    Metrics,
    Orderbook,
    TestEngine(EngineMode),
    Load { paths: Vec<String> },
    Simulate { path: String, interval_secs: Option<u64> },
    Download(DownloadTarget),
    Extract(DownloadTarget),
    Help,
    Exit,
}

impl CliCommand {
    fn parse(input: &str) -> Option<Self> {
        // Defensive: some Windows pipelines (e.g. PowerShell piping a file
        // into this process's stdin) prepend a UTF-8 BOM to the very first
        // line only. `str::trim()` doesn't strip it (U+FEFF isn't
        // whitespace), so without this the first piped command would
        // silently fail to match any command name.
        let input = input.trim_start_matches('\u{feff}');
        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        match parts[0].to_lowercase().as_str() {
            "add" => {
                if parts.len() < 5 {
                    println!(" Usage: add <buy|sell> <price|market> <qty> <user>");
                    return None;
                }
                let side = match parts[1].to_lowercase().as_str() {
                    "buy" => Side::Buy,
                    "sell" => Side::Sell,
                    _ => return None,
                };
                // Both accept decimals now (e.g. "127.06"), not just whole
                // numbers — reuses the exact same pure-integer parsers the
                // CSV loader uses, so `add`-typed and `load`-sourced orders
                // are parsed identically. `price` comes out already
                // PRICE_SCALE-scaled (see `simulator::parse_fixed_point`);
                // `qty` is rounded to the nearest whole unit (see
                // `simulator::round_to_unit` for why — the engine has no
                // fixed-point convention for quantity, only price).
                let price = if parts[2].eq_ignore_ascii_case("market") {
                    None
                } else {
                    Some(simulator::parse_fixed_point(parts[2])?)
                };
                let qty = simulator::round_to_unit(parts[3])?;
                let user = parts[4].to_string();
                Some(CliCommand::Add { side, price, qty, user })
            }
            "engine" => {
                if parts.len() < 2 {
                    println!(" Usage: engine <continuous|batch>");
                    return None;
                }
                match parts[1].to_lowercase().as_str() {
                    "continuous" | "cda" => Some(CliCommand::Engine(EngineMode::Continuous)),
                    "batch" | "fba" => Some(CliCommand::Engine(EngineMode::Batch)),
                    _ => {
                        println!(" Unknown engine type. Choose 'continuous' or 'batch'.");
                        None
                    }
                }
            }
            "batch" => Some(CliCommand::Batch),
            "clear" => Some(CliCommand::Clear),
            "log" => Some(CliCommand::Log),
            "metrics" | "stats" => Some(CliCommand::Metrics),
            "orderbook" | "ob" => Some(CliCommand::Orderbook),
            "test" => {
                if parts.len() < 3 || parts[1].to_lowercase() != "engine" {
                    println!(" Usage: test engine <continuous|batch>");
                    return None;
                }
                match parts[2].to_lowercase().as_str() {
                    "continuous" | "cda" => Some(CliCommand::TestEngine(EngineMode::Continuous)),
                    "batch" | "fba" => Some(CliCommand::TestEngine(EngineMode::Batch)),
                    _ => {
                        println!(" Unknown engine type. Choose 'continuous' or 'batch'.");
                        None
                    }
                }
            }
            "load" => {
                if parts.len() < 2 {
                    println!(" Usage: load <path> [path...]");
                    return None;
                }
                let paths = parts[1..].iter().map(|s| s.to_string()).collect();
                Some(CliCommand::Load { paths })
            }
            "simulate" => {
                if parts.len() < 2 {
                    println!(" Usage: simulate <path> [interval_seconds]");
                    return None;
                }
                let path = parts[1].to_string();
                let interval_secs = match parts.get(2) {
                    Some(raw) => match raw.parse::<u64>() {
                        Ok(v) => Some(v),
                        Err(_) => {
                            println!(" Invalid interval_seconds: '{raw}'");
                            return None;
                        }
                    },
                    None => None,
                };
                Some(CliCommand::Simulate { path, interval_secs })
            }
            "download" => {
                if parts.len() < 2 {
                    println!(" Usage: download <btc|eth|sol|all>");
                    return None;
                }
                parse_download_target(parts[1]).map(CliCommand::Download)
            }
            "extract" => {
                if parts.len() < 2 {
                    println!(" Usage: extract <btc|eth|sol|all>");
                    return None;
                }
                parse_download_target(parts[1]).map(CliCommand::Extract)
            }
            "help" => Some(CliCommand::Help),
            "exit" | "quit" => Some(CliCommand::Exit),
            _ => None,
        }
    }
}

/// Shared coin-argument parsing for `download`/`extract` — both take the
/// exact same `<btc|eth|sol|all>` argument and print the same usage hint on
/// an unrecognized one.
fn parse_download_target(arg: &str) -> Option<DownloadTarget> {
    match arg.to_lowercase().as_str() {
        "btc" => Some(DownloadTarget::Coin(Coin::Btc)),
        "eth" => Some(DownloadTarget::Coin(Coin::Eth)),
        "sol" => Some(DownloadTarget::Coin(Coin::Sol)),
        "all" => Some(DownloadTarget::All),
        _ => {
            println!(" Unknown coin. Choose 'btc', 'eth', 'sol', or 'all'.");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_add_limit_order() {
        // price comes out already PRICE_SCALE-scaled (127 -> 127_000_000),
        // matching every other internal price representation in this crate.
        assert_eq!(
            CliCommand::parse("add buy 127 5 Alice"),
            Some(CliCommand::Add { side: Side::Buy, price: Some(127_000_000), qty: 5, user: "Alice".to_string() })
        );
        assert_eq!(
            CliCommand::parse("add sell 130 3 Bob"),
            Some(CliCommand::Add { side: Side::Sell, price: Some(130_000_000), qty: 3, user: "Bob".to_string() })
        );
    }

    #[test]
    fn parses_add_with_decimal_price_and_quantity() {
        // The actual bug report this fixes: decimals in price/qty (real
        // SOL prices/sizes, e.g. from data/sample) used to fail to parse
        // at all — only whole numbers worked.
        assert_eq!(
            CliCommand::parse("add buy 127.06 5.5 Alice"),
            // 127.06 -> 127_060_000 (PRICE_SCALE-scaled); 5.5 rounds to 6
            // (round-half-up — see simulator::round_to_unit, the same
            // rounding the CSV loader uses for fractional order sizes).
            Some(CliCommand::Add { side: Side::Buy, price: Some(127_060_000), qty: 6, user: "Alice".to_string() })
        );
        assert_eq!(
            CliCommand::parse("add sell 126.4 2.3 Bob"),
            Some(CliCommand::Add { side: Side::Sell, price: Some(126_400_000), qty: 2, user: "Bob".to_string() })
        );
    }

    #[test]
    fn parses_add_market_order() {
        // This is the exact command that surfaced real user confusion:
        // `price: None` here is what tells `run()` to build a market
        // order (`Order::market`) instead of a limit order.
        assert_eq!(
            CliCommand::parse("add buy market 15 999"),
            Some(CliCommand::Add { side: Side::Buy, price: None, qty: 15, user: "999".to_string() })
        );
        // Case-insensitive, matching every other keyword this parser accepts.
        assert_eq!(
            CliCommand::parse("add sell MARKET 4 Charlie"),
            Some(CliCommand::Add { side: Side::Sell, price: None, qty: 4, user: "Charlie".to_string() })
        );
    }

    #[test]
    fn rejects_malformed_add_commands() {
        assert_eq!(CliCommand::parse("add buy 127"), None); // too few args
        assert_eq!(CliCommand::parse("add hold 127 5 Alice"), None); // not buy/sell
        assert_eq!(CliCommand::parse("add buy notanumber 5 Alice"), None); // price isn't a number or "market"
        assert_eq!(CliCommand::parse("add buy 127 notanumber Alice"), None); // qty isn't a number
    }

    #[test]
    fn parses_engine_switch_and_its_aliases() {
        assert_eq!(CliCommand::parse("engine continuous"), Some(CliCommand::Engine(EngineMode::Continuous)));
        assert_eq!(CliCommand::parse("engine cda"), Some(CliCommand::Engine(EngineMode::Continuous)));
        assert_eq!(CliCommand::parse("engine batch"), Some(CliCommand::Engine(EngineMode::Batch)));
        assert_eq!(CliCommand::parse("engine fba"), Some(CliCommand::Engine(EngineMode::Batch)));
        assert_eq!(CliCommand::parse("engine nonsense"), None);
        assert_eq!(CliCommand::parse("engine"), None); // missing the mode argument
    }

    #[test]
    fn parses_simple_no_argument_commands() {
        assert_eq!(CliCommand::parse("batch"), Some(CliCommand::Batch));
        assert_eq!(CliCommand::parse("clear"), Some(CliCommand::Clear));
        assert_eq!(CliCommand::parse("log"), Some(CliCommand::Log));
        assert_eq!(CliCommand::parse("metrics"), Some(CliCommand::Metrics));
        assert_eq!(CliCommand::parse("stats"), Some(CliCommand::Metrics)); // alias
        assert_eq!(CliCommand::parse("orderbook"), Some(CliCommand::Orderbook));
        assert_eq!(CliCommand::parse("ob"), Some(CliCommand::Orderbook)); // alias
        assert_eq!(CliCommand::parse("help"), Some(CliCommand::Help));
        assert_eq!(CliCommand::parse("exit"), Some(CliCommand::Exit));
        assert_eq!(CliCommand::parse("quit"), Some(CliCommand::Exit)); // alias
    }

    #[test]
    fn parses_load_with_one_or_more_paths() {
        assert_eq!(CliCommand::parse("load a.csv"), Some(CliCommand::Load { paths: vec!["a.csv".to_string()] }));
        assert_eq!(
            CliCommand::parse("load a.csv b.csv"),
            Some(CliCommand::Load { paths: vec!["a.csv".to_string(), "b.csv".to_string()] })
        );
        assert_eq!(CliCommand::parse("load"), None); // needs at least one path
    }

    #[test]
    fn parses_simulate_with_and_without_an_interval_override() {
        assert_eq!(
            CliCommand::parse("simulate ../data/order_statuses/20251201"),
            Some(CliCommand::Simulate { path: "../data/order_statuses/20251201".to_string(), interval_secs: None })
        );
        assert_eq!(
            CliCommand::parse("simulate ../data/order_statuses/20251201/sol_00.data.gz 60"),
            Some(CliCommand::Simulate { path: "../data/order_statuses/20251201/sol_00.data.gz".to_string(), interval_secs: Some(60) })
        );
        assert_eq!(CliCommand::parse("simulate"), None); // needs at least a path
        assert_eq!(CliCommand::parse("simulate some/path notanumber"), None); // interval must be numeric
    }

    #[test]
    fn parses_download_command_and_rejects_unknown_coins() {
        assert_eq!(CliCommand::parse("download btc"), Some(CliCommand::Download(DownloadTarget::Coin(Coin::Btc))));
        assert_eq!(CliCommand::parse("download eth"), Some(CliCommand::Download(DownloadTarget::Coin(Coin::Eth))));
        assert_eq!(CliCommand::parse("download sol"), Some(CliCommand::Download(DownloadTarget::Coin(Coin::Sol))));
        assert_eq!(CliCommand::parse("download all"), Some(CliCommand::Download(DownloadTarget::All)));
        assert_eq!(CliCommand::parse("download SOL"), Some(CliCommand::Download(DownloadTarget::Coin(Coin::Sol)))); // case-insensitive
        assert_eq!(CliCommand::parse("download doge"), None); // not one of the three supported coins
        assert_eq!(CliCommand::parse("download"), None); // missing the coin argument
    }

    #[test]
    fn parses_extract_command_and_rejects_unknown_coins() {
        // Same argument shape as `download` — both go through
        // `parse_download_target`.
        assert_eq!(CliCommand::parse("extract btc"), Some(CliCommand::Extract(DownloadTarget::Coin(Coin::Btc))));
        assert_eq!(CliCommand::parse("extract all"), Some(CliCommand::Extract(DownloadTarget::All)));
        assert_eq!(CliCommand::parse("extract SOL"), Some(CliCommand::Extract(DownloadTarget::Coin(Coin::Sol)))); // case-insensitive
        assert_eq!(CliCommand::parse("extract doge"), None); // not one of the three supported coins
        assert_eq!(CliCommand::parse("extract"), None); // missing the coin argument
    }

    #[test]
    fn parses_test_engine_command_and_its_aliases() {
        assert_eq!(CliCommand::parse("test engine continuous"), Some(CliCommand::TestEngine(EngineMode::Continuous)));
        assert_eq!(CliCommand::parse("test engine cda"), Some(CliCommand::TestEngine(EngineMode::Continuous)));
        assert_eq!(CliCommand::parse("test engine batch"), Some(CliCommand::TestEngine(EngineMode::Batch)));
        assert_eq!(CliCommand::parse("test engine fba"), Some(CliCommand::TestEngine(EngineMode::Batch)));
        assert_eq!(CliCommand::parse("test"), None); // missing "engine <mode>"
        assert_eq!(CliCommand::parse("test something continuous"), None); // wrong second word
    }

    #[test]
    fn rejects_unknown_commands() {
        assert_eq!(CliCommand::parse("frobnicate"), None);
        assert_eq!(CliCommand::parse(""), None);
        assert_eq!(CliCommand::parse("   "), None);
    }

    #[test]
    fn strips_leading_bom_before_matching_the_command_name() {
        // Regression test: some Windows pipelines (PowerShell piping a
        // file into this process's stdin) prepend a UTF-8 BOM to the very
        // first line only. This used to make the first piped command
        // silently fail to parse — see the comment on `parse` above.
        assert_eq!(
            CliCommand::parse("\u{feff}load a.csv"),
            Some(CliCommand::Load { paths: vec!["a.csv".to_string()] })
        );
        assert_eq!(CliCommand::parse("\u{feff}clear"), Some(CliCommand::Clear));
    }
}

pub fn run() {
    let mut fba = FbaOrderBook::new();
    let mut cda = CdaOrderBook::new();
    let mut order_id_counter: u64 = 1;
    let mut current_mode = EngineMode::Batch;

    let rule = "======================================================================".cyan();
    println!("{rule}");
    println!("{}", "TFG Simulator".cyan().bold());
    println!("{rule}");
    print_help();

    loop {
        let mode_label = match current_mode {
            EngineMode::Continuous => "CDA".blue().bold(),
            EngineMode::Batch => "FBA".yellow().bold(),
        };

        print!("\n{} [{mode_label}]{} ", "sim".dimmed(), ">".dimmed());
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }

        match CliCommand::parse(&input) {
            Some(CliCommand::Engine(mode)) => {
                current_mode = mode;
                println!("{}", format!("[OK] Switched simulation matching engine mode to: {current_mode:?}").cyan());
            }

            Some(CliCommand::Add { side, price, qty, user }) => {
                let timestamp = now_ns();
                let assigned_id = order_id_counter;
                // `price` is already PRICE_SCALE-scaled by the parser
                // (`simulator::parse_fixed_point`) — no scaling needed here.
                let order = match price {
                    Some(internal_price) => Order::limit(assigned_id, user, side, internal_price, qty, timestamp),
                    None => Order::market(assigned_id, user, side, qty, timestamp),
                };
                order_id_counter += 1;

                match current_mode {
                    EngineMode::Batch => {
                        fba.submit(order);
                        println!("{}", format!("[OK] Queued order successfully in FBA discrete window buffer [ID: {assigned_id}]").green());
                    }
                    EngineMode::Continuous => {
                        let trades = cda.submit(order);
                        println!("{}", format!("[OK] Continuous order processed. Instant trades cleared: {}", trades.len()).green());
                        for t in &trades {
                            println!("   [TRADE] Qty: {} at Price: {}", t.quantity, format_price(t.price));
                        }
                    }
                }
            }

            Some(CliCommand::Batch) => match current_mode {
                EngineMode::Continuous => render_book(&cda),
                EngineMode::Batch => render_pending(&fba),
            },

            Some(CliCommand::Clear) => {
                if current_mode == EngineMode::Continuous {
                    println!("{}", "[WARN] Continuous engine processes trades instantly. 'clear' applies to the FBA pipeline.".yellow());
                }
                render_clear(&mut fba);
            }

            Some(CliCommand::Log) => render_log(&fba, &cda),

            Some(CliCommand::Metrics) => stats::print_summary(&fba, &cda),

            Some(CliCommand::Orderbook) => match current_mode {
                EngineMode::Batch => {
                    render_pending(&fba);
                    stats::print_fba(&fba);
                }
                EngineMode::Continuous => {
                    render_book(&cda);
                    stats::print_cda(&cda);
                }
            },

            Some(CliCommand::TestEngine(EngineMode::Continuous)) => {
                let cases = test_suite::run_cda_tests();
                test_suite::print_checklist("CDA", &cases);
            }

            Some(CliCommand::TestEngine(EngineMode::Batch)) => {
                let cases = test_suite::run_fba_tests();
                test_suite::print_checklist("FBA", &cases);
            }

            Some(CliCommand::Load { paths }) => {
                let mut total = 0usize;
                let mut live = 0usize;

                for path in &paths {
                    match simulator::load_order_status_csv(path) {
                        Ok(orders) => {
                            for order in orders {
                                total += 1;
                                if order.is_new_live_order() {
                                    live += 1;
                                }
                                match current_mode {
                                    EngineMode::Batch => fba.submit(order),
                                    EngineMode::Continuous => {
                                        cda.submit(order);
                                    }
                                }
                            }
                        }
                        Err(err) => println!("{}", format!("[ERROR] Failed to load '{path}': {err}").red()),
                    }
                }

                // Not wrapped in a single color: `mode_label` is itself
                // colored (see the prompt above), and nesting a `colored`
                // string inside another resets the outer color partway
                // through the line — better to let the FBA/CDA token pop
                // on its own against plain text here.
                println!("[OK] Loaded {total} order-status record(s) ({live} live) into the {mode_label} engine.");
            }

            Some(CliCommand::Simulate { path, interval_secs }) => {
                simulate_cmd::run(&path, interval_secs);
            }

            Some(CliCommand::Download(target)) => download_cmd::run(target),

            Some(CliCommand::Extract(target)) => download_cmd::run_extract(target),

            Some(CliCommand::Help) => print_help(),

            Some(CliCommand::Exit) => {
                println!("{}", "Terminating simulator core workspace...".dimmed());
                break;
            }

            None => println!("{}", "[ERROR] Command sequence unrecognized. Run 'help' to review syntax specifications.".red()),
        }
    }
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

fn print_help() {
    println!("\n{}", "Available Simulation Interaction Inputs:".bold());
    const ROWS: &[(&str, &str)] = &[
        ("engine <continuous|batch>", "Dynamically flip between matching engine paradigms"),
        ("add <buy|sell> <price|market> <qty> <user>", "Submit a limit order (numeric price) or a market order ('market') to the active engine (SOL/USD)"),
        ("batch", "Inspect active continuous book state or FBA buffer state"),
        ("clear", "Force close batch window, clear matching equations, and log data"),
        ("load <path> [path...]", "Replay order-status CSV file(s) (data/SCHEMA.md PREVIEW format) into the active engine"),
        ("simulate <path|btc|eth|sol|all> [interval_secs]", "Replay real .csv/.gz order-status data (a file/folder, 'btc'/'eth'/'sol' for data/order_statuses/<coin>/, or 'all' for all three in sequence) through BOTH engines, write time-series metrics to output/"),
        ("download <btc|eth|sol|all>", "Fetch that coin's order-status archive from Zenodo and extract it into data/order_statuses/<coin>/, ready for 'simulate <coin>'"),
        ("extract <btc|eth|sol|all>", "(Re-)extract an already-downloaded archive without re-fetching it — for when 'download's extraction step failed but the archive is already local"),
        ("log", "Audit chronological ledger (combined FBA + CDA executed trades)"),
        ("metrics", "Print core metrics computed so far, for both engines"),
        ("orderbook", "Print the active engine's orderbook state + its own core metrics, in one view"),
        ("test engine <continuous|batch>", "Run the built-in test checklist against a fresh, isolated instance of that engine"),
        ("help", "Review configuration tools"),
        ("exit", "Safely close terminal stream"),
    ];
    for (cmd, desc) in ROWS {
        // Pad the plain text first, then color it — coloring first would
        // wrap the string in invisible ANSI escape bytes that `{:<45}`
        // would count toward the padding width, throwing off alignment.
        let padded = format!("{cmd:<45}");
        println!("  {} - {}", padded.cyan(), desc.dimmed());
    }
}

// ---- display helpers ----

fn format_price(price: u128) -> String {
    let whole = price / PRICE_SCALE;
    let fractional = price % PRICE_SCALE;
    format!("{whole}.{fractional:06}")
}

fn render_pending(fba: &FbaOrderBook) {
    println!("{}", "---  Current Pending FBA Window Accumulation Buffer ---".bold());

    let best_buy = fba.best_unfilled_buy().map(format_price).unwrap_or_else(|| "n/a".to_string());
    let best_sell = fba.best_unfilled_sell().map(format_price).unwrap_or_else(|| "n/a".to_string());
    println!("Best Unfilled Buy: {}  |  Best Unfilled Sell: {}", best_buy.green(), best_sell.red());

    if fba.pending_orders.is_empty() {
        println!("(No orders currently inside this discrete window buffer)");
        return;
    }

    // Split by side and sort best-first (highest buy limit / lowest sell
    // limit) so the best bid/ask is always the first row printed under
    // each heading, matching the summary line above and the CDA book
    // view below. Market orders (no limit price) don't compete on price
    // for this display, so they sort last on their side.
    let mut buys: Vec<&Order> = fba.pending_orders.iter().filter(|o| o.side() == Side::Buy).collect();
    let mut sells: Vec<&Order> = fba.pending_orders.iter().filter(|o| o.side() == Side::Sell).collect();
    buys.sort_by_key(|o| std::cmp::Reverse(o.limit_price().unwrap_or(0)));
    sells.sort_by_key(|o| o.limit_price().unwrap_or(u128::MAX));

    // Buy/sell get their own color throughout — green for the bid side,
    // red for the ask side, the same convention most trading terminals use.
    println!("{}", format!("Buy orders ({}):", buys.len()).green().bold());
    for o in &buys {
        let price_str = o.limit_price().map_or("MARKET".to_string(), format_price);
        println!("{}", format!("  ID: {:<3} | User: {:<8} | Qty: {:<4} | Max Limit: {} USDT", o.oid, o.user_id, o.remaining, price_str).green());
    }
    println!("{}", format!("Sell orders ({}):", sells.len()).red().bold());
    for o in &sells {
        let price_str = o.limit_price().map_or("MARKET".to_string(), format_price);
        println!("{}", format!("  ID: {:<3} | User: {:<8} | Qty: {:<4} | Max Limit: {} USDT", o.oid, o.user_id, o.remaining, price_str).red());
    }
}

fn render_book(cda: &CdaOrderBook) {
    println!("{}", "---  Current Continuous Order Book State ---".bold());

    let best_bid = cda.best_bid().map(format_price).unwrap_or_else(|| "n/a".to_string());
    let best_ask = cda.best_ask().map(format_price).unwrap_or_else(|| "n/a".to_string());
    let spread = cda.quoted_spread_bps().map(|v| format!("{v:.2} bps")).unwrap_or_else(|| "n/a".to_string());
    println!("Best Bid: {}  |  Best Ask: {}  |  Spread: {spread}", best_bid.green(), best_ask.red());

    // `bids`/`asks` are always kept sorted best-first (see submit()'s
    // sort_by calls), so no re-sort needed here — the first row under
    // each heading below IS the best bid / best ask from the summary line.
    // Bids get green (the buy side), asks get red (the sell side) — same
    // convention render_pending uses for FBA's buy/sell orders.
    println!("{}", format!("Bids ({}):", cda.bids.len()).green().bold());
    for o in &cda.bids {
        println!("{}", format!("  ID: {:<3} | User: {:<8} | Qty: {:<4} | Price: {}", o.oid, o.user_id, o.remaining, format_price(o.limit_px)).green());
    }
    println!("{}", format!("Asks ({}):", cda.asks.len()).red().bold());
    for o in &cda.asks {
        println!("{}", format!("  ID: {:<3} | User: {:<8} | Qty: {:<4} | Price: {}", o.oid, o.user_id, o.remaining, format_price(o.limit_px)).red());
    }
}

fn render_clear(fba: &mut FbaOrderBook) {
    if fba.pending_orders.is_empty() {
        println!("{}", "[WARN] Window buffer is empty. No discrete allocations can clear!".yellow());
        return;
    }

    println!("\n{}", "[FBA Window Closed] Computing Uniform Market Clearing...".cyan());
    println!("{}", "==============================================================".cyan());

    match fba.clear() {
        Some(clearing) => {
            println!("{}", "[OK] Uniform Clearing Calculated Successfully!".green());
            println!("   Execution Rate (Uniform Price) : {} USDT", format_price(clearing.clearing_price));
            println!("   Total Executed Asset Mass       : {} units", clearing.traded_quantity);

            println!("\nDetailed Execution Trade Log:");
            if clearing.trades.is_empty() {
                println!("   (No trades matched within this crossover threshold)");
            } else {
                for trade in &clearing.trades {
                    println!(
                        "   Match ID #{:<3} | {} (Order #{}) bought {} units from {} (Order #{}) @ {} USDT",
                        trade.trade_id,
                        trade.buyer_id,
                        trade.buy_order_id,
                        trade.quantity,
                        trade.seller_id,
                        trade.sell_order_id,
                        format_price(trade.price)
                    );
                }
            }

            let unfilled = fba.pending_orders.len();
            if unfilled > 0 {
                println!("{}", format!("\n[INFO] {unfilled} order(s) left unexecuted at this clearing price — rolled over to the next window.").yellow());
            }
        }
        None => {
            println!("{}", "[ERROR] Convergence Failure: No mathematical crossover found inside this batch window.".red());
        }
    }
}

fn render_log(fba: &FbaOrderBook, cda: &CdaOrderBook) {
    let rule = "==========================================================================".cyan();
    println!("\n{rule}");
    println!("{}", "                      SYSTEM HISTORICAL EXECUTION LOG                     ".cyan().bold());
    println!("{rule}");

    let mut combined: Vec<&Trade> = fba.executed_trades.iter().chain(cda.executed_trades.iter()).collect();
    combined.sort_by_key(|t| t.ts);

    if combined.is_empty() {
        println!("  No trades have executed yet.");
    } else {
        println!(
            "  {}",
            format!("{:<10} | {:<4} | {:<12} | {:<12} | {:<10} | {:<12}", "Trade ID", "Eng", "Buyer", "Seller", "Quantity", "Price").bold()
        );
        println!("  ------------------------------------------------------------------------");
        for t in combined {
            // Pad the plain label first, then color it — see print_help's
            // comment on why coloring before padding breaks alignment.
            let engine_padded = format!("{:<4}", engine_label(t.engine_type));
            let engine = match t.engine_type {
                EngineKind::Fba => engine_padded.yellow(),
                EngineKind::Cda => engine_padded.blue(),
            };
            println!(
                "  #{:<9} | {} | {:<12} | {:<12} | {:<10} | {:<12}",
                t.trade_id,
                engine,
                t.buyer_id,
                t.seller_id,
                t.quantity,
                format_price(t.price)
            );
        }
    }
    println!("{rule}\n");
}

fn engine_label(kind: EngineKind) -> &'static str {
    kind.label()
}
