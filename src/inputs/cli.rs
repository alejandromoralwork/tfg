//! Interactive command-prompt loop for driving both engines and testing
//! them by hand or by replaying `data/sample`. Ported from the old
//! `simulation` crate's `commands.rs` + `main.rs` + `display.rs`, folded
//! into one file here since there's no need for three anymore.

use std::io::{self, Write};

use crate::engines::cda::CdaOrderBook;
use crate::engines::fba::FbaOrderBook;
use crate::inputs::simulator;
use crate::inputs::test_suite;
use crate::metrics::stats;
use crate::types::{EngineKind, Order, Side, Trade, PRICE_SCALE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineMode {
    Continuous,
    Batch,
}

enum CliCommand {
    // `price: None` means a market order (fills at the resting/eligible
    // book's own price rather than a limit the caller sets).
    Add { side: Side, price: Option<u128>, qty: u128, user: String },
    Engine(EngineMode),
    Batch,
    Clear,
    Log,
    Metrics,
    Orderbook,
    TestEngine(EngineMode),
    Load { paths: Vec<String> },
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
                let price = if parts[2].eq_ignore_ascii_case("market") {
                    None
                } else {
                    Some(parts[2].parse::<u128>().ok()?)
                };
                let qty = parts[3].parse::<u128>().ok()?;
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
            "help" => Some(CliCommand::Help),
            "exit" | "quit" => Some(CliCommand::Exit),
            _ => None,
        }
    }
}

pub fn run() {
    let mut fba = FbaOrderBook::new();
    let mut cda = CdaOrderBook::new();
    let mut order_id_counter: u64 = 1;
    let mut current_mode = EngineMode::Batch;

    println!("======================================================================");
    println!("🚀 Cross-Paradigm Market Research Simulator Core");
    println!("   Trading Pair: SOL/USD (this simulation trades a single fixed pair)");
    println!("======================================================================");
    print_help();

    loop {
        let mode_label = match current_mode {
            EngineMode::Continuous => "CDA",
            EngineMode::Batch => "FBA",
        };

        print!("\nsim [{mode_label}]> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }

        match CliCommand::parse(&input) {
            Some(CliCommand::Engine(mode)) => {
                current_mode = mode;
                println!("🔄 Switched simulation matching engine mode to: {current_mode:?}");
            }

            Some(CliCommand::Add { side, price, qty, user }) => {
                let timestamp = now_ns();
                let assigned_id = order_id_counter;
                let order = match price {
                    Some(raw_price) => {
                        let internal_price = raw_price * PRICE_SCALE;
                        Order::limit(assigned_id, user, side, internal_price, qty, timestamp)
                    }
                    None => Order::market(assigned_id, user, side, qty, timestamp),
                };
                order_id_counter += 1;

                match current_mode {
                    EngineMode::Batch => {
                        fba.submit(order);
                        println!("✅ Queued order successfully in FBA discrete window buffer [ID: {assigned_id}]");
                    }
                    EngineMode::Continuous => {
                        let trades = cda.submit(order);
                        println!("⚡ Continuous order processed. Instant trades cleared: {}", trades.len());
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
                    println!("⚠️ Info: Continuous engine processes trades instantly. 'clear' applies to the FBA pipeline.");
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
                        Err(err) => println!("❌ Failed to load '{path}': {err}"),
                    }
                }

                println!("📥 Loaded {total} order-status record(s) ({live} live) into the {mode_label} engine.");
            }

            Some(CliCommand::Help) => print_help(),

            Some(CliCommand::Exit) => {
                println!("Terminating simulator core workspace...");
                break;
            }

            None => println!("❌ Command sequence unrecognized. Run 'help' to review syntax specifications."),
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
    println!("\nAvailable Simulation Interaction Inputs:");
    println!("  engine <continuous|batch>           - Dynamically flip between matching engine paradigms");
    println!("  add <buy|sell> <price|market> <qty> <user> - Submit a limit order (numeric price) or a market order ('market') to the active engine (SOL/USD)");
    println!("  batch                               - Inspect active continuous book state or FBA buffer state");
    println!("  clear                               - Force close batch window, clear matching equations, and log data");
    println!("  load <path> [path...]               - Replay order-status CSV file(s) (data/SCHEMA.md PREVIEW format) into the active engine");
    println!("  log                                 - Audit chronological ledger (combined FBA + CDA executed trades)");
    println!("  metrics                             - Print core metrics computed so far, for both engines");
    println!("  orderbook (alias: ob)                - Print the active engine's orderbook state + its own core metrics, in one view");
    println!("  test engine <continuous|batch>      - Run the built-in test checklist against a fresh, isolated instance of that engine");
    println!("  help                                - Review configuration tools");
    println!("  exit                                - Safely close terminal stream");
}

// ---- display helpers ----

fn format_price(price: u128) -> String {
    let whole = price / PRICE_SCALE;
    let fractional = price % PRICE_SCALE;
    format!("{whole}.{fractional:06}")
}

fn render_pending(fba: &FbaOrderBook) {
    println!("---  Current Pending FBA Window Accumulation Buffer ---");
    if fba.pending_orders.is_empty() {
        println!("(No orders currently inside this discrete window buffer)");
    } else {
        for o in &fba.pending_orders {
            let side_str = if o.side() == Side::Buy { "BUY " } else { "SELL" };
            let price_str = o.limit_price().map_or("MARKET".to_string(), format_price);
            println!(
                "ID: {:<3} | User: {:<8} | {} | Qty: {:<4} | Max Limit: {} USDT",
                o.oid, o.user_id, side_str, o.remaining, price_str
            );
        }
    }
}

fn render_book(cda: &CdaOrderBook) {
    println!("---  Current Continuous Order Book State ---");
    println!("Bids ({}):", cda.bids.len());
    for o in &cda.bids {
        println!("  ID: {:<3} | User: {:<8} | Qty: {:<4} | Price: {}", o.oid, o.user_id, o.remaining, format_price(o.limit_px));
    }
    println!("Asks ({}):", cda.asks.len());
    for o in &cda.asks {
        println!("  ID: {:<3} | User: {:<8} | Qty: {:<4} | Price: {}", o.oid, o.user_id, o.remaining, format_price(o.limit_px));
    }
}

fn render_clear(fba: &mut FbaOrderBook) {
    if fba.pending_orders.is_empty() {
        println!("⚠️ Window buffer is empty. No discrete allocations can clear!");
        return;
    }

    println!("\n🔄 [FBA Window Closed] Computing Uniform Market Clearing...");
    println!("==============================================================");

    match fba.clear() {
        Some(clearing) => {
            println!("✅ Uniform Clearing Calculated Successfully!");
            println!("   Execution Rate (Uniform Price) : {} USDT", format_price(clearing.clearing_price));
            println!("   Total Executed Asset Mass       : {} units", clearing.traded_quantity);

            println!("\n📜 Detailed Execution Trade Log:");
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
                println!("\n⏭️  {unfilled} order(s) left unexecuted at this clearing price — rolled over to the next window.");
            }
        }
        None => {
            println!("❌ Convergence Failure: No mathematical crossover found inside this batch window.");
        }
    }
}

fn render_log(fba: &FbaOrderBook, cda: &CdaOrderBook) {
    println!("\n==========================================================================");
    println!("📜                     SYSTEM HISTORICAL EXECUTION LOG                    ");
    println!("==========================================================================");

    let mut combined: Vec<&Trade> = fba.executed_trades.iter().chain(cda.executed_trades.iter()).collect();
    combined.sort_by_key(|t| t.ts);

    if combined.is_empty() {
        println!("  No trades have executed yet.");
    } else {
        println!("  {:<10} | {:<4} | {:<12} | {:<12} | {:<10} | {:<12}", "Trade ID", "Eng", "Buyer", "Seller", "Quantity", "Price");
        println!("  ------------------------------------------------------------------------");
        for t in combined {
            println!(
                "  #{:<9} | {:<4} | {:<12} | {:<12} | {:<10} | {:<12}",
                t.trade_id,
                engine_label(t.engine_type),
                t.buyer_id,
                t.seller_id,
                t.quantity,
                format_price(t.price)
            );
        }
    }
    println!("==========================================================================\n");
}

fn engine_label(kind: EngineKind) -> &'static str {
    kind.label()
}
