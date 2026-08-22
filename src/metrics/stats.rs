//! Thin orchestrator: no calculation logic of its own, just calls each
//! engine's own orderbook methods and prints them side by side. Each
//! orderbook is responsible for computing its own numbers from its own
//! state (see `engines::fba::FbaOrderBook` / `engines::cda::CdaOrderBook`).

use crate::engines::cda::CdaOrderBook;
use crate::engines::fba::FbaOrderBook;

fn fmt_opt(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.4}"),
        None => "n/a".to_string(),
    }
}

pub fn print_summary(fba: &FbaOrderBook, cda: &CdaOrderBook) {
    println!("\n==========================================================================");
    println!("📈                              CORE METRICS                              ");
    println!("==========================================================================");
    print_fba(fba);
    print_cda(cda);
    println!("==========================================================================\n");
}

/// Just the FBA block — reused by both `print_summary` (both engines) and
/// the `orderbook` CLI command (active engine only).
pub fn print_fba(fba: &FbaOrderBook) {
    println!("\n--- FBA ---");
    println!("  quoted_spread_bps        : {}", fmt_opt(fba.quoted_spread_bps()));
    println!("  depth_at_best            : {}", fba.depth_at_best());
    println!("  trade_count              : {}", fba.trade_count());
    println!("  executed_volume          : {}", fba.executed_volume());
    println!("  executed_notional        : {}", fba.executed_notional());
    println!("  fill_rate                : {}", fmt_opt(fba.fill_rate()));
    println!("  unexecuted_residual_share: {}", fmt_opt(fba.unexecuted_residual_share()));
}

/// Just the CDA block — reused by both `print_summary` (both engines) and
/// the `orderbook` CLI command (active engine only).
pub fn print_cda(cda: &CdaOrderBook) {
    println!("\n--- CDA ---");
    println!("  quoted_spread_bps        : {}", fmt_opt(cda.quoted_spread_bps()));
    println!("  depth_at_best            : {}", cda.depth_at_best());
    println!("  trade_count              : {}", cda.trade_count());
    println!("  executed_volume          : {}", cda.executed_volume());
    println!("  executed_notional        : {}", cda.executed_notional());
    println!("  fill_rate                : {}", fmt_opt(cda.fill_rate()));
    println!("  book_imbalance           : {}", fmt_opt(cda.book_imbalance()));
}
