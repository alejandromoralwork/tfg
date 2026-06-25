use engines::common::Side;
use crate::commands::{CliCommand, EngineMode}; // 🟢 Local import

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
                if parts.len() < 2 { return None; }
                match parts[1].to_lowercase().as_str() {
                    "continuous" | "cda" => Some(CliCommand::Engine { mode: EngineMode::Continuous }),
                    "batch" | "fba" => Some(CliCommand::Engine { mode: EngineMode::Batch }),
                    _ => None,
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