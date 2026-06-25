use engines::common::Side;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineMode {
    Continuous,
    Batch,
}

pub enum CliCommand {
    Add { side: Side, asset: String, price: u128, qty: u128, user: String },
    Engine(EngineMode), // New command variant
    Batch,
    Amm,
    Clear,
    Log,
    Help,
    Exit,
}

impl CliCommand {
    pub fn parse(input: &str) -> Option<Self> {
        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        if parts.is_empty() { return None; }

        match parts[0].to_lowercase().as_str() {
            "add" => {
                if parts.len() < 6 { return None; }
                let side = match parts[1].to_lowercase().as_str() {
                    "buy" => Side::Buy,
                    "sell" => Side::Sell,
                    _ => return None,
                };
                let asset = parts[2].to_uppercase();
                let price = parts[3].parse::<u128>().ok()?;
                let qty = parts[4].parse::<u128>().ok()?;
                let user = parts[5].to_string();
                
                Some(CliCommand::Add { side, asset, price, qty, user })
            }
            "engine" => {
                if parts.len() < 2 { 
                    println!("❌ Usage: engine <continuous|batch>");
                    return None; 
                }
                match parts[1].to_lowercase().as_str() {
                    "continuous" | "cda" => Some(CliCommand::Engine(EngineMode::Continuous)),
                    "batch" | "fba" => Some(CliCommand::Engine(EngineMode::Batch)),
                    _ => {
                        println!("❌ Unknown engine type. Choose 'continuous' or 'batch'.");
                        None
                    }
                }
            }
            "batch" => Some(CliCommand::Batch),
            "amm" => Some(CliCommand::Amm),
            "clear" => Some(CliCommand::Clear),
            "log" => Some(CliCommand::Log),
            "help" => Some(CliCommand::Help),
            "exit" | "quit" => Some(CliCommand::Exit),
            _ => None,
        }
    }
}