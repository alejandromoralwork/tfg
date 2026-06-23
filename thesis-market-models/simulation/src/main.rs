use std::io::{self, Write};

// 🟢 Declare simulation specific modules cleanly
mod commands; 
mod cli; 
mod simulator;
mod display;

use commands::{CliCommand, EngineMode}; // 🟢 Pull from local modules
use simulator::FbaSimulator;

use engines::fba::BatchAuctionEngine; 

// In simulation/src/main.rs

use engines::cda::ContinuousEngine; // 🟢 Matches your crate structure perfectly now!

use engines::common::Order;

fn main() {
    let mut sim = FbaSimulator::new("USDT");
    let mut cda = ContinuousEngine::new();
    let mut current_mode = EngineMode::Batch;
    
    println!("======================================================================");
    println!("🚀 Cross-Paradigm Market Research Simulator Core");
    println!("   Reference Anchor Currency: USDT");
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
            Some(CliCommand::Engine { mode }) => {
                current_mode = mode;
                println!("🔄 Switched simulation matching engine mode to: {:?}", current_mode);
            }
            Some(CliCommand::Add { side, asset, price, qty, user }) => {
                match current_mode {
                    EngineMode::Batch => {
                        let id = sim.add_order(side, &asset, price, qty, user);
                        println!("✅ Queued order successfully in FBA discrete window buffer [ID: {}]", id);
                    }
                    EngineMode::Continuous => {
                        sim.order_id_counter += 1;
                        let pair = engines::common::AssetPair::new(&asset, "USDT");
                        let order = Order::limit(sim.order_id_counter, &user, pair, side, price * engines::common::PRICE_SCALE, qty, 0);
                        
                        let trades = cda.process_order(order);
                        println!("⚡ Continuous order processed. Instant trades cleared: {}", trades.len());
                        for t in trades {
                            println!("   [TRADE] Qty: {} at Price: {}", t.quantity, t.price / engines::common::PRICE_SCALE);
                        }
                    }
                }
            }
            Some(CliCommand::Batch) => {
                if current_mode == EngineMode::Continuous {
                    println!("📖 Displaying Continuous Order Book state:\n{:#?}", cda);
                } else {
                    display::render_batch_buffer(&sim);
                }
            }
            Some(CliCommand::Amm)   => display::render_amm_matrices(&sim),
            Some(CliCommand::Clear) => {
                if current_mode == EngineMode::Continuous {
                    println!("⚠️ Info: Continuous engine processes trades instantly. 'clear' applies to the FBA pipeline.");
                }
                sim.clear_window();
            }
            Some(CliCommand::Log)   => display::render_historical_ledger(&sim),
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
    println!("  add <buy|sell> <asset> <price> <qty> <user> - Commit limit liquidity parameters to active engine");
    println!("  batch                               - Inspect active continuous book state or FBA buffer state");
    println!("  amm                                 - Snapshot the multi-currency liquidity reserves");
    println!("  clear                               - Force close batch window, clear matching equations, and log data");
    println!("  log                                 - Audit chronological ledger (System wide orders, P2P, and AMM swaps)");
    println!("  help                                - Review configuration tools");
    println!("  exit                                - Safely close terminal stream");
}