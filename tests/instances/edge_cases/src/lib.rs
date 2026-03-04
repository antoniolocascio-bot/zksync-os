//! User-facing edge-case tests for ZKsync OS.

#![cfg(test)]

use rig::alloy::primitives::{address, U256 as AlloyU256};
use rig::alloy::signers::local::PrivateKeySigner;
use rig::builder::TxBuilder;
use rig::constants::*;
use rig::ruint::aliases::U256;
use rig::run_config;
use rig::TestingFramework;
use rig::{assert_gas_used_lt, assert_nonce, assert_tx_success};

fn new_tester() -> TestingFramework<false> {
    TestingFramework::new().with_run_config(run_config::forward_only())
}

#[test]
fn zero_value_transfer_to_eoa() {
    let signer = PrivateKeySigner::random();
    let sender = signer.address();
    let recipient = address!("deadbeef00000000000000000000000000000001");

    let mut tester = new_tester().with_balance(sender, U256::from(DEFAULT_BALANCE));
    let tx = TxBuilder::new()
        .from(signer)
        .to(recipient)
        .value(AlloyU256::ZERO)
        .gas_limit(TRANSFER_GAS_LIMIT)
        .build();
    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);
}

#[test]
fn self_transfer_succeeds() {
    let signer = PrivateKeySigner::random();
    let sender = signer.address();

    let mut tester = new_tester().with_balance(sender, U256::from(DEFAULT_BALANCE));
    let tx = TxBuilder::new()
        .from(signer)
        .to(sender)
        .value(AlloyU256::from(1_000u64))
        .gas_limit(TRANSFER_GAS_LIMIT)
        .build();

    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);
}

#[test]
fn empty_calldata_call_to_contract() {
    let return_bytecode = hex::decode("60006000f3").unwrap();
    let contract = address!("0000000000000000000000000000000000000401");

    let signer = PrivateKeySigner::random();
    let sender = signer.address();

    let mut tester = new_tester()
        .with_balance(sender, U256::from(DEFAULT_BALANCE))
        .with_evm_contract(contract, &return_bytecode);

    let tx = TxBuilder::new()
        .from(signer)
        .to(contract)
        .gas_limit(CALL_GAS_LIMIT)
        .build();
    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);
}

#[test]
fn multi_tx_block_state_dependency() {
    let contract_bytecode =
        hex::decode("36600014600d5760ab600055005b60005460005260206000f3").unwrap();
    let contract_addr = address!("0000000000000000000000000000000000000501");

    let signer1 = PrivateKeySigner::random();
    let signer2 = PrivateKeySigner::random();
    let sender1 = signer1.address();
    let sender2 = signer2.address();

    let mut tester = new_tester()
        .with_balance(sender1, U256::from(DEFAULT_BALANCE))
        .with_balance(sender2, U256::from(DEFAULT_BALANCE))
        .with_evm_contract(contract_addr, &contract_bytecode);

    let tx0 = TxBuilder::new()
        .from(signer1)
        .to(contract_addr)
        .calldata(vec![0x01])
        .gas_limit(CALL_GAS_LIMIT)
        .build();
    let tx1 = TxBuilder::new()
        .from(signer2)
        .to(contract_addr)
        .gas_limit(CALL_GAS_LIMIT)
        .build();

    let output = tester.execute_block(vec![tx0, tx1]);
    assert_tx_success!(output, 0);
    assert_tx_success!(output, 1);

    let tx1_out = output.tx_results[1].as_ref().unwrap();
    let returned = tx1_out.as_returned_bytes();
    let mut expected = [0u8; 32];
    expected[31] = 0xAB;
    assert_eq!(returned, &expected);
}

#[test]
fn state_persists_across_blocks() {
    let contract_bytecode =
        hex::decode("36600014600d5760be600055005b60005460005260206000f3").unwrap();
    let contract = address!("0000000000000000000000000000000000000601");

    let signer = PrivateKeySigner::random();
    let sender = signer.address();

    let mut tester = new_tester()
        .with_balance(sender, U256::from(DEFAULT_BALANCE))
        .with_evm_contract(contract, &contract_bytecode);

    let tx1 = TxBuilder::new()
        .from(signer.clone())
        .to(contract)
        .calldata(vec![0x01])
        .gas_limit(CALL_GAS_LIMIT)
        .build();
    let out1 = tester.execute_block(vec![tx1]);
    assert_tx_success!(out1, 0);

    let tx2 = TxBuilder::new()
        .from(signer)
        .to(contract)
        .nonce(1)
        .gas_limit(CALL_GAS_LIMIT)
        .build();
    let out2 = tester.execute_block(vec![tx2]);
    assert_tx_success!(out2, 0);

    let tx2_out = out2.tx_results[0].as_ref().unwrap();
    let returned = tx2_out.as_returned_bytes();
    let mut expected = [0u8; 32];
    expected[31] = 0xBE;
    assert_eq!(returned, &expected);
}

#[test]
fn transfer_gas_within_bounds() {
    let signer = PrivateKeySigner::random();
    let sender = signer.address();
    let recipient = address!("deadbeef00000000000000000000000000000002");

    let mut tester = new_tester().with_balance(sender, U256::from(DEFAULT_BALANCE));
    let tx = TxBuilder::new()
        .from(signer)
        .to(recipient)
        .gas_limit(TRANSFER_GAS_LIMIT)
        .build();

    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);
    assert_gas_used_lt!(output, 0, 400_000);
}

#[test]
fn call_to_stop_contract_succeeds() {
    let stop_bytecode = hex::decode("00").unwrap();
    let contract = address!("0000000000000000000000000000000000000701");

    let signer = PrivateKeySigner::random();
    let sender = signer.address();

    let mut tester = new_tester()
        .with_balance(sender, U256::from(DEFAULT_BALANCE))
        .with_evm_contract(contract, &stop_bytecode);

    let tx = TxBuilder::new()
        .from(signer)
        .to(contract)
        .gas_limit(CALL_GAS_LIMIT)
        .build();

    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);
}

#[test]
fn nonce_incremented_after_success() {
    let signer = PrivateKeySigner::random();
    let sender = signer.address();
    let recipient = address!("deadbeef00000000000000000000000000000003");

    let mut tester = new_tester().with_balance(sender, U256::from(DEFAULT_BALANCE));
    let tx0 = TxBuilder::new()
        .from(signer.clone())
        .to(recipient)
        .nonce(0)
        .gas_limit(TRANSFER_GAS_LIMIT)
        .build();
    let out0 = tester.execute_block(vec![tx0]);
    assert_tx_success!(out0, 0);
    assert_nonce!(tester, sender, 1);

    let tx1 = TxBuilder::new()
        .from(signer)
        .to(recipient)
        .nonce(1)
        .gas_limit(TRANSFER_GAS_LIMIT)
        .build();
    let out1 = tester.execute_block(vec![tx1]);
    assert_tx_success!(out1, 0);
    assert_nonce!(tester, sender, 2);
}

#[test]
fn large_calldata_does_not_panic() {
    let return_bytecode = hex::decode("60006000f3").unwrap();
    let contract = address!("0000000000000000000000000000000000000801");
    let large_calldata = vec![0u8; 32 * 1024];

    let signer = PrivateKeySigner::random();
    let sender = signer.address();

    let mut tester = new_tester()
        .with_balance(sender, U256::from(DEFAULT_BALANCE))
        .with_evm_contract(contract, &return_bytecode);

    let tx = TxBuilder::new()
        .from(signer)
        .to(contract)
        .calldata(large_calldata)
        .gas_limit(5_000_000)
        .build();
    let output = tester.execute_block(vec![tx]);
    match &output.tx_results[0] {
        Ok(_) | Err(_) => {}
    }
}

#[test]
fn tstore_tload_same_tx() {
    let bytecode = hex::decode("60ab60005d60005c60005260206000f3").unwrap();
    let contract = address!("0000000000000000000000000000000000000901");

    let signer = PrivateKeySigner::random();
    let sender = signer.address();

    let mut tester = new_tester()
        .with_balance(sender, U256::from(DEFAULT_BALANCE))
        .with_evm_contract(contract, &bytecode);
    let tx = TxBuilder::new()
        .from(signer)
        .to(contract)
        .gas_limit(CALL_GAS_LIMIT)
        .build();
    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);

    let tx_out = output.tx_results[0].as_ref().unwrap();
    let returned = tx_out.as_returned_bytes();
    let mut expected = [0u8; 32];
    expected[31] = 0xAB;
    assert_eq!(returned, &expected);
}

#[test]
fn tstore_cleared_between_txs() {
    let contract_bytecode =
        hex::decode("36600014600e5761dead60005d005b60005c60005260206000f3").unwrap();
    let contract_addr = address!("0000000000000000000000000000000000000902");

    let signer1 = PrivateKeySigner::random();
    let signer2 = PrivateKeySigner::random();
    let sender1 = signer1.address();
    let sender2 = signer2.address();

    let mut tester = new_tester()
        .with_balance(sender1, U256::from(DEFAULT_BALANCE))
        .with_balance(sender2, U256::from(DEFAULT_BALANCE))
        .with_evm_contract(contract_addr, &contract_bytecode);

    let tx0 = TxBuilder::new()
        .from(signer1)
        .to(contract_addr)
        .calldata(vec![0x01])
        .gas_limit(CALL_GAS_LIMIT)
        .build();
    let tx1 = TxBuilder::new()
        .from(signer2)
        .to(contract_addr)
        .gas_limit(CALL_GAS_LIMIT)
        .build();

    let output = tester.execute_block(vec![tx0, tx1]);
    assert_tx_success!(output, 0);
    assert_tx_success!(output, 1);

    let tx1_out = output.tx_results[1].as_ref().unwrap();
    let returned = tx1_out.as_returned_bytes();
    assert_eq!(returned, &[0u8; 32]);
}

#[test]
fn difficulty_opcode_matches_current_runtime_semantics() {
    let bytecode = hex::decode("4460005260206000f3").unwrap();
    let contract = address!("0000000000000000000000000000000000000a01");

    let signer = PrivateKeySigner::random();
    let sender = signer.address();

    let mut tester = new_tester()
        .with_balance(sender, U256::from(DEFAULT_BALANCE))
        .with_evm_contract(contract, &bytecode);

    let tx = TxBuilder::new()
        .from(signer)
        .to(contract)
        .gas_limit(CALL_GAS_LIMIT)
        .build();
    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);

    let tx_out = output.tx_results[0].as_ref().unwrap();
    let returned = tx_out.as_returned_bytes();
    let mut expected = [0u8; 32];
    // `zk_ee::System::get_mix_hash` currently returns constant `1` unless built with
    // `prevrandao` feature enabled.
    expected[31] = 1;
    assert_eq!(returned, &expected);
}

#[test]
fn coinbase_visible_in_contract() {
    let bytecode = hex::decode("4160005260206000f3").unwrap();
    let contract = address!("0000000000000000000000000000000000000b01");
    let coinbase_addr = address!("1234567890abcdef1234567890abcdef12345678");

    let signer = PrivateKeySigner::random();
    let sender = signer.address();

    let mut tester = new_tester()
        .with_balance(sender, U256::from(DEFAULT_BALANCE))
        .with_evm_contract(contract, &bytecode)
        .with_block_context(rig::chain::BlockContext {
            coinbase: rig::ruint::aliases::B160::from_be_bytes(coinbase_addr.into_array()),
            ..rig::chain::BlockContext::default()
        });

    let tx = TxBuilder::new()
        .from(signer)
        .to(contract)
        .gas_limit(CALL_GAS_LIMIT)
        .build();
    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);

    let tx_out = output.tx_results[0].as_ref().unwrap();
    let returned = tx_out.as_returned_bytes();
    let mut expected = [0u8; 32];
    expected[12..].copy_from_slice(&coinbase_addr.into_array());
    assert_eq!(returned, &expected);
}

#[test]
fn l1_tx_accepted() {
    let signer = PrivateKeySigner::random();
    let sender = signer.address();
    let recipient = address!("deadbeef00000000000000000000000000000099");

    let mut tester = new_tester().with_balance(sender, U256::from(DEFAULT_BALANCE));
    let tx = TxBuilder::new()
        .l1()
        .from(signer)
        .to(recipient)
        .gas_limit(TRANSFER_GAS_LIMIT)
        .build();

    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);
}

#[test]
fn log4_event_has_four_topics() {
    let mut bytecode: Vec<u8> = Vec::new();
    bytecode.push(0x7f);
    bytecode.extend_from_slice(&[0x01u8; 32]);
    bytecode.push(0x7f);
    bytecode.extend_from_slice(&[0x02u8; 32]);
    bytecode.push(0x7f);
    bytecode.extend_from_slice(&[0x03u8; 32]);
    bytecode.push(0x7f);
    bytecode.extend_from_slice(&[0x04u8; 32]);
    bytecode.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0xa4]);
    bytecode.push(0x00);

    let contract = address!("0000000000000000000000000000000000000c01");
    let signer = PrivateKeySigner::random();
    let sender = signer.address();

    let mut tester = new_tester()
        .with_balance(sender, U256::from(DEFAULT_BALANCE))
        .with_evm_contract(contract, &bytecode);
    let tx = TxBuilder::new()
        .from(signer)
        .to(contract)
        .gas_limit(CALL_GAS_LIMIT)
        .build();
    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);

    let tx_out = output.tx_results[0].as_ref().unwrap();
    assert!(!tx_out.logs.is_empty());
    let log = &tx_out.logs[0];
    assert_eq!(log.topics().len(), 4);
    assert_eq!(log.topics()[0].0, [0x04u8; 32]);
    assert_eq!(log.topics()[1].0, [0x03u8; 32]);
    assert_eq!(log.topics()[2].0, [0x02u8; 32]);
    assert_eq!(log.topics()[3].0, [0x01u8; 32]);
}

#[test]
fn account_diffs_after_eth_transfer() {
    let signer = PrivateKeySigner::random();
    let sender = signer.address();
    let recipient = address!("deadbeef00000000000000000000000000000042");

    let mut tester = new_tester().with_balance(sender, U256::from(DEFAULT_BALANCE));
    let tx = TxBuilder::new()
        .from(signer)
        .to(recipient)
        .value(AlloyU256::from(1_000u64))
        .gas_limit(TRANSFER_GAS_LIMIT)
        .build();

    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);

    let recipient_diff = output.account_diffs.iter().find(|d| d.address == recipient);
    assert!(
        recipient_diff.is_some(),
        "expected account_diff entry for recipient {recipient:?}"
    );
}
