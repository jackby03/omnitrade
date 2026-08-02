//! # omnitrade-cli
//!
//! Headless command-line interface for backtesting strategies and running
//! automated integration tests without a GUI.

mod args;
mod backtest;

use args::{CliArgs, Commands};
use backtest::{run_backtest, BacktestConfig};
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
            let config = BacktestConfig::new(
                script,
                data,
                initial_balance,
                fee_rate,
                slippage_bps,
            );

            let result = run_backtest(&config)?;
            println!("=== Backtest Results ===");
            println!("Total PnL:       {:.2}", result.total_pnl);
            println!("Final Balance:   {:.2}", result.final_balance);
            println!("Total Trades:    {}", result.total_trades);
            println!("Win Rate:        {:.2}%", result.win_rate * 100.0);
            println!("Max Drawdown:    {:.2}%", result.max_drawdown * 100.0);
        }
        Commands::Validate { script } => {
            let script_content = std::fs::read_to_string(&script)?;
            let tokens = omnitrade_script::tokenize(&script_content)?;
            let _stmts = omnitrade_script::parse(&tokens)?;
            println!("Script validation successful: {:?}", script);
        }
    }

    Ok(())
}
