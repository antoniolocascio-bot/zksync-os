//! ETH STF witness generation debug tool.
//! Can be used to debug Ethereum STF witness generation locally.
//! See `super::Command::EthStfWitGen` for more details.

use crate::block::Block;
use crate::live_run::rpc::{self, JsonResponse};
use alloy::consensus::Header;
use alloy::eips::Typed2718;
use alloy::primitives::U256;
use rig::alloy_rlp::Encodable;
use anyhow::Context;

use rig::log::{info, warn};
use rig::zksync_os_tests_common::zksync_tx::encoding::encode_alloy_rpc_tx;
use rig::zksync_os_interface::traits::EncodedTx;
use rig::*;

use std::collections::HashMap;
use std::fs::File;
use std::thread::sleep;
use std::time::Duration;

fn get_witness_from_file(
    block_dir: &str,
) -> anyhow::Result<(u64, Block, alloy_rpc_types_debug::ExecutionWitness)> {
    use std::fs;
    use std::path::Path;

    let dir = Path::new(&block_dir);
    let block = fs::read_to_string(dir.join("block.json"))?;
    let witness = fs::File::open(dir.join("witness.json"))?;

    let block: Block = serde_json::from_str(&block).context("Failed to parse block.json")?;
    let block_number = block.result.header.number;

    let rpc_result: JsonResponse<alloy_rpc_types_debug::ExecutionWitness> =
        serde_json::from_reader(witness)?;
    Ok((block_number, block, rpc_result.result))
}

fn get_witness_from_reth(
    reth_endpoint: &str,
    block_number: String,
    save: bool,
) -> anyhow::Result<(u64, Block, alloy_rpc_types_debug::ExecutionWitness)> {
    let block_number: u64 = match block_number.as_str() {
        "latest" => rpc::get_block_number(reth_endpoint)?,
        n if n.starts_with("0x") => {
            u64::from_str_radix(&n[2..], 16).context("Failed to parse hex block number")?
        }
        n if n.parse::<u64>().is_ok() => n.parse().unwrap(),
        _ => anyhow::bail!("Invalid block number format"),
    };
    log::info!("Resolved block number: {}", block_number);

    let block = rpc::get_block(reth_endpoint, block_number)?;
    let witness = rpc::get_witness(reth_endpoint, block_number)?;

    if save {
        let folder = format!("tests/instances/eth_runner/blocks/{}", block_number);
        if std::path::Path::new(&folder).exists() {
            warn!(
                "Block directory {} already exists, ignoring save attribute",
                folder
            );
        } else {
            std::fs::create_dir_all(&folder)
                .context("Failed to create block directory")?;
            serde_json::to_writer_pretty(
                File::create(format!("{}/block.json", folder))
                    .context("Failed to create block.json")?,
                &block,
            )
            .context("Failed to write block.json")?;
            serde_json::to_writer_pretty(
                File::create(format!("{}/witness.json", folder))
                    .context("Failed to create witness.json")?,
                &witness.result,
            )
            .context("Failed to write witness.json")?;
            log::info!("Saved block and witness to {}", folder);
        }
    }

    log::info!("Fetched info from reth endpoint");
    Ok((block_number, block, witness.result))
}

pub fn run_cont(reth_endpoint: String) -> anyhow::Result<()> {
    let mut current_block = rpc::get_block_number(&reth_endpoint)?;

    loop {
        let res = run(None, reth_endpoint.clone(), Some(current_block.to_string()), false);
        if res.is_ok() {
            log::info!("Block {current_block} -> OK");
        } else {
            log::error!("Block {current_block} -> ERR: {:?}", res.err());
        }

        current_block += 1;
        while rpc::get_block_number(&reth_endpoint)? < current_block {
            sleep(Duration::from_secs(1));
        }
    }
}

/// Runs ETH STF witness generation for a given block.
pub fn run(
    block_dir: Option<String>,
    reth_endpoint: String,
    block_number: Option<String>,
    save: bool,
) -> anyhow::Result<()> {
    let (block_number, block, witness) = match (block_dir, block_number) {
        (Some(dir), None) => get_witness_from_file(&dir)?,
        (None, Some(num)) => get_witness_from_reth(&reth_endpoint, num, save)?,
        _ => {
            anyhow::bail!("Either block_dir or block_number must be provided, but not both");
        }
    };

    let mut tx_types: HashMap<u8, usize> = HashMap::new();
    for tx in block.result.transactions.as_transactions().unwrap() {
        let tx_type = tx.ty();
        *tx_types.entry(tx_type).or_insert(0) += 1;
    }
    for (tx_type, count) in tx_types.iter() {
        let tx_ty_str = alloy::consensus::TxType::try_from(*tx_type)
            .unwrap()
            .to_string();
        info!("Transaction type {tx_ty_str}: {count} transactions");
    }

    let current_time = std::time::SystemTime::now();

    // Decode headers from witness and build block hash chain
    let mut headers: Vec<Header> = witness
        .headers
        .iter()
        .map(|el| alloy_rlp::decode_exact(&el[..]).expect("must decode headers from witness"))
        .collect();
    assert!(!headers.is_empty());
    assert!(headers.is_sorted_by(|a, b| { a.number < b.number }));
    headers.reverse();

    assert_eq!(headers[0].number, block_number - 1);
    let mut block_hashes: Vec<U256> = headers
        .iter()
        .map(|el| U256::from_be_bytes(el.hash_slow().0))
        .collect();
    block_hashes.resize(256, U256::ZERO);

    info!("Running block: {block_number}");
    info!("Block gas used: {}", block.result.header.gas_used);

    let header = block.result.header.clone().into();
    let withdrawals_encoding = if let Some(withdrawals) = block.result.withdrawals.clone() {
        let mut buff = vec![];
        withdrawals.encode(&mut buff);
        buff
    } else {
        Vec::new()
    };

    let transactions: Vec<EncodedTx> = block
        .result
        .transactions
        .clone()
        .into_transactions()
        .map(encode_alloy_rpc_tx)
        .collect();

    let mut chain = Chain::empty(Some(1));
    chain.set_last_block_number(block_number - 1);
    chain.set_block_hashes(block_hashes.try_into().unwrap());

    let (_result_keeper, _witness_output) = chain.run_eth_block_with_options(
        transactions,
        witness,
        header,
        withdrawals_encoding,
        None,
        true, // only_forward
    );

    let duration = current_time.elapsed().unwrap();
    info!("Time taken: {:?}", duration);
    Ok(())
}
