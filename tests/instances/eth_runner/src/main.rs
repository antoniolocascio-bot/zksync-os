#![recursion_limit = "1024"]

use clap::{Parser, Subcommand};
mod block;
mod block_hashes;
mod calltrace;
mod eth_stf_witgen;
mod live_run;
mod native_model;
mod post_check;
mod prestate;
mod receipts;
mod single_run;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {}

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a range of blocks live from RPC
    LiveRun {
        #[arg(long)]
        start_block: u64,
        #[arg(long)]
        end_block: u64,
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        db: String,
        #[arg(long)]
        witness_output_dir: Option<String>,
        #[arg(long)]
        skip_successful: bool,
        #[arg(long)]
        persist_all: bool,
        #[arg(long)]
        slack_webhook: Option<String>,
        #[arg(long)]
        single_tx: Option<u64>,
        #[arg(long)]
        only_forward: bool,
        #[arg(long)]
        backup_endpoint: Option<String>,
    },
    // Run a single block from JSON files
    SingleRun {
        /// Path to the block JSON file
        #[arg(long)]
        block_dir: String,
        /// Path to the block hashes JSON file (optional)
        #[arg(long)]
        block_hashes: Option<String>,
        /// If set, the leaves of the tree are put in random
        /// positions to emulate real-world costs
        #[arg(long, action = clap::ArgAction::SetTrue)]
        randomized: bool,
        /// If set, will run prover input generation and dump it
        /// to the desired path.
        #[arg(long)]
        witness_output_dir: Option<String>,
        #[arg(long)]
        chain_id: Option<u64>,
        #[arg(long)]
        single_tx: Option<u64>,
    },
    // Export block ratios from DB
    ExportRatios {
        #[arg(long)]
        db: String,
        #[arg(long)]
        path: Option<String>,
    },
    // Show failed blocks
    ShowStatus {
        #[arg(long)]
        db: String,
    },
    // Run a single block using eth_run
    EthRun {
        /// Path to the block directory
        #[arg(long)]
        block_dir: String,
    },
    /// Run ETH STF witness generation for debugging.
    /// Fetches block and execution witness from L1, and attempts to generate witness.
    EthStfWitGen {
        /// Path to a local block directory (block.json + witness.json)
        #[arg(long)]
        block_dir: Option<String>,
        /// Save fetched data to disk
        #[arg(long)]
        save: bool,
        /// Block number to fetch (decimal, hex with 0x prefix, or "latest")
        #[arg(long)]
        block_number: Option<String>,
        /// Reth RPC endpoint URL
        #[arg(long)]
        reth_endpoint: String,
        /// Run continuously, processing blocks in sequence
        #[arg(long)]
        cont: bool,
    },
}

fn main() -> anyhow::Result<()> {
    rig::init_logger();
    let cli = Cli::parse();
    match cli.command {
        Command::SingleRun {
            block_dir,
            block_hashes,
            randomized,
            witness_output_dir,
            chain_id,
            single_tx,
        } => crate::single_run::single_run(
            block_dir,
            block_hashes,
            randomized,
            witness_output_dir,
            chain_id,
            single_tx,
        ),
        Command::LiveRun {
            start_block,
            end_block,
            endpoint,
            db,
            witness_output_dir,
            skip_successful,
            persist_all,
            slack_webhook,
            single_tx,
            only_forward,
            backup_endpoint,
        } => live_run::live_run(
            start_block,
            end_block,
            endpoint,
            db,
            witness_output_dir,
            skip_successful,
            persist_all,
            slack_webhook,
            single_tx,
            only_forward,
            backup_endpoint,
        ),
        Command::ExportRatios { db, path } => live_run::export_block_ratios(db, path),
        Command::ShowStatus { db } => live_run::show_status(db),
        Command::EthRun { block_dir } => crate::single_run::eth_run(block_dir),
        Command::EthStfWitGen {
            block_dir,
            block_number,
            reth_endpoint,
            save,
            cont,
        } => {
            if cont {
                crate::eth_stf_witgen::run_cont(reth_endpoint)?;
            } else {
                crate::eth_stf_witgen::run(block_dir, reth_endpoint, block_number, save)?;
            }
            Ok(())
        }
    }
}
