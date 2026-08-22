use engines::common::Side;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineMode {
    Continuous,
    Batch,
}

pub enum CliCommand {
    // This simulation only trades the single default SOL/USD pair, so orders
    // no longer carry an asset argument.
    Add { side: Side, price: u128, qty: u128, user: String },
    Engine(EngineMode), // New command variant
    Batch,
    Clear,
    Log,
    Metrics,
    // One or more order-status CSV paths (see data/SCHEMA.md's PREVIEW
    // format) to replay into whichever engine is currently active.
    Load { paths: Vec<String> },
    Help,
    Exit,
}

impl CliCommand {
    pub fn parse(input: &str) -> Option<Self> {
        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        if parts.is_empty() { return None; }

        match parts[0].to_lowercase().as_str() {
            "add" => {
                if parts.len() < 5 { return None; }
                let side = match parts[1].to_lowercase().as_str() {
                    "buy" => Side::Buy,
                    "sell" => Side::Sell,
                    _ => return None,
                };
                let price = parts[2].parse::<u128>().ok()?;
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