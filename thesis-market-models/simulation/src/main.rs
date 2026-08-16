use std::io::{self, Write};


mod commands;
mod simulator;
mod cda_simulator;
mod depth;
mod display;


use crate::commands::{CliCommand, EngineMode};
use crate::simulator::FbaSimulator;
use crate::cda_simulator::CdaSimulator;

/// Bucket width for the metric time series, in nanoseconds (matches the L4
/// dataset's own timestamp convention — see metrics::collector docs).
/// 1 second is a sensible default for interactive CLI experimentation; a
/// real replay run should set this to whatever batch interval tau it uses.
const DEFAULT_INTERVAL_NS: u64 = 1_000_000_000;

fn main() {
    let mut sim = FbaSimulator::new(DEFAULT_INTERVAL_NS);
    let mut cda = CdaSimulator::new(DEFAULT_INTERVAL_NS);
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

        print!("\nsim [{}]> ", mode_label);
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() { break; }

        match CliCommand::parse(&input) {
            Some(CliCommand::Engine(mode)) => {
                current_mode = mode;
                println!("🔄 Switched simulation matching engine mode to: {:?}", current_mode);
            }

            Some(CliCommand::Add { side, price, qty, user }) => {
                match current_mode {
                    EngineMode::Batch => {
                        let id = sim.add_order(side, price, qty, user);
                        println!("✅ Queued order successfully in FBA discrete window buffer [ID: {}]", id);
                    }
                    EngineMode::Continuous => {
                        let trades = cda.add_order(side, price, qty, user);
                        println!("⚡ Continuous order processed. Instant trades cleared: {}", trades.len());
                        for t in trades {
                            println!("   [TRADE] Qty: {} at Price: {}", t.quantity, t.price / engines::common::PRICE_SCALE);
                        }
                    }
                }
            }

            Some(CliCommand::Batch) => {
                if current_mode == EngineMode::Continuous {
                    println!("📖 Displaying Continuous Order Book state:\n{:#?}", cda.engine);
                } else {
                    display::render_batch_buffer(&sim);
                }
            }

            Some(CliCommand::Clear) => {
                if current_mode == EngineMode::Continuous {
                    println!("⚠️ Info: Continuous engine processes trades instantly. 'clear' applies to the FBA pipeline.");
                }
                sim.clear_window();
            }

            Some(CliCommand::Log)   => display::render_historical_ledger(&sim),

            Some(CliCommand::Metrics) => display::render_metrics(&sim, &cda),

            Some(CliCommand::Help)  => print_help(),

            Some(CliCommand::Exit)  => {
                println!("Terminating simulator core workspace...");
                break;
            }

            None => println!("❌ Command sequence unrecognized. Run 'help' to review syntax specifications."),
        }
    }
}

fn print_help() {
    println!("\nAvailable Simulation Interaction Inputs:");
    println!("  engine <continuous|batch>           - Dynamically flip between matching engine paradigms");
    println!("  add <buy|sell> <price> <qty> <user> - Commit limit liquidity parameters to active engine (SOL/USD)");
    println!("  batch                               - Inspect active continuous book state or FBA buffer state");
    println!("  clear                               - Force close batch window, clear matching equations, and log data");
    println!("  log                                 - Audit chronological ledger (System wide orders and P2P clearing trades)");
    println!("  metrics                             - Print the RQ2 metric time series computed so far, for both engines");
    println!("  help                                - Review configuration tools");
    println!("  exit                                - Safely close terminal stream");
}
