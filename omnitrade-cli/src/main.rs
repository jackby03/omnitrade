//! # omnitrade-cli
//!
//! Headless command-line interface for backtesting strategies and running
//! automated integration tests without a GUI.

mod args;

use args::{CliArgs, Commands};
use clap::Parser;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = CliArgs::parse();

    match args.command {
        Commands::Backtest {
            script,
            data,
            initial_balance,
            fee_rate,
            slippage_bps,
        } => {
            println!(
                "Running backtest: script={:?}, data={:?}, initial_balance={}, fee_rate={}, slippage_bps={}",
                script, data, initial_balance, fee_rate, slippage_bps
            );
        }
        Commands::Validate { script } => {
            println!("Validating script: {:?}", script);
        }
    }

    Ok(())
}
