mod types;
mod engines;
mod inputs;
mod metrics;

fn main() {
    // With no arguments, start the interactive prompt (the normal desktop
    // use). With arguments, run that one command and exit with its status —
    // the non-interactive path for scripting and GCP Batch, e.g.
    //   market_sim simulate sol 1
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        inputs::cli::run();
    } else {
        std::process::exit(inputs::cli::run_once(&args));
    }
}
