//! Fluent builders for test framework setup and transaction construction.

use crate::chain::RunConfig;
use crate::constants::{CALL_GAS_LIMIT, DEFAULT_MAX_FEE, DEFAULT_PRIORITY_FEE, TEST_CHAIN_ID};
use crate::TestingFramework;
use alloy::consensus::{TxEip1559, TxEip2930, TxLegacy};
use alloy::eips::eip2930::AccessList;
use alloy::primitives::{Address, Bytes, TxKind, B256, U256};
use alloy::signers::local::PrivateKeySigner;
use ruint::aliases::{B160, B256 as RuintB256, U256 as RuintU256};
use zk_ee::utils::Bytes32;
use zksync_os_tests_common::zksync_tx::l1_tx::ZKsyncL1Tx;
use zksync_os_tests_common::zksync_tx::upgrade_tx::ZKsyncUpgradeTx;
use zksync_os_tests_common::zksync_tx::ZKsyncTxEnvelope;

/// Fluent builder for [`TestingFramework<false>`].
///
/// This is a small wrapper over existing `TestingFramework` builder methods, meant to keep test
/// setup concise and readable.
pub struct ChainBuilder {
    framework: TestingFramework<false>,
}

impl ChainBuilder {
    /// Start a new chain builder with default testing settings.
    pub fn new() -> Self {
        Self {
            framework: TestingFramework::new(),
        }
    }

    /// Override chain ID.
    pub fn chain_id(mut self, chain_id: u64) -> Self {
        self.framework = self.framework.with_chain_id(chain_id);
        self
    }

    /// Fund an account.
    pub fn with_balance(mut self, address: Address, amount: RuintU256) -> Self {
        self.framework = self.framework.with_balance(address, amount);
        self
    }

    /// Fund an account (B160 overload).
    pub fn with_balance_b160(self, address: B160, amount: RuintU256) -> Self {
        self.with_balance(Address::from(address.to_be_bytes::<20>()), amount)
    }

    /// Deploy EVM bytecode at `address`.
    pub fn with_evm_bytecode(mut self, address: Address, bytecode: Vec<u8>) -> Self {
        self.framework = self.framework.with_evm_contract(address, &bytecode);
        self
    }

    /// Deploy EVM bytecode at `address` (B160 overload).
    pub fn with_evm_bytecode_b160(self, address: B160, bytecode: Vec<u8>) -> Self {
        self.with_evm_bytecode(Address::from(address.to_be_bytes::<20>()), bytecode)
    }

    /// Set a storage slot.
    pub fn with_storage_slot(mut self, address: Address, key: RuintU256, value: RuintB256) -> Self {
        self.framework = self.framework.with_storage_slot(address, key, value);
        self
    }

    /// Register a preimage.
    pub fn with_preimage(mut self, hash: Bytes32, data: Vec<u8>) -> Self {
        self.framework = self.framework.with_preimage(hash, &data);
        self
    }

    /// Install selected system contracts.
    pub fn with_system_contracts(
        mut self,
        with_l1_messenger: bool,
        with_l2_base_token: bool,
        with_contract_deployer: bool,
    ) -> Self {
        self.framework = self.framework.with_system_contracts(
            with_l1_messenger,
            with_l2_base_token,
            with_contract_deployer,
        );
        self
    }

    /// Override run configuration.
    pub fn with_run_config(mut self, run_config: RunConfig) -> Self {
        self.framework = self.framework.with_run_config(run_config);
        self
    }

    /// Consume the builder and return a configured testing framework.
    pub fn build(self) -> TestingFramework<false> {
        self.framework
    }
}

impl Default for ChainBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Supported transaction kinds for [`TxBuilder`].
#[derive(Debug, Clone, Copy, Default)]
pub enum TxType {
    #[default]
    Eip1559,
    Legacy,
    Eip2930,
    L1,
    Upgrade,
}

/// Fluent builder for [`ZKsyncTxEnvelope`].
pub struct TxBuilder {
    tx_type: TxType,
    chain_id: u64,
    signer: Option<PrivateKeySigner>,
    from: Option<Address>,
    to: TxKind,
    calldata: Vec<u8>,
    value: U256,
    gas_limit: u64,
    nonce: u64,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    access_list: AccessList,
    gas_per_pubdata_byte_limit: u128,
    refund_recipient: Option<Address>,
    to_mint: Option<U256>,
    factory_deps: Vec<B256>,
}

impl TxBuilder {
    /// Create a new builder defaulting to EIP-1559 with sensible test values.
    pub fn new() -> Self {
        Self {
            tx_type: TxType::Eip1559,
            chain_id: TEST_CHAIN_ID,
            signer: None,
            from: None,
            to: TxKind::Call(Address::ZERO),
            calldata: Vec::new(),
            value: U256::ZERO,
            gas_limit: CALL_GAS_LIMIT,
            nonce: 0,
            max_fee_per_gas: DEFAULT_MAX_FEE,
            max_priority_fee_per_gas: DEFAULT_PRIORITY_FEE,
            access_list: AccessList::default(),
            gas_per_pubdata_byte_limit: 0,
            refund_recipient: None,
            to_mint: None,
            factory_deps: Vec::new(),
        }
    }

    /// Build an EIP-1559 transaction.
    pub fn eip1559(mut self) -> Self {
        self.tx_type = TxType::Eip1559;
        self
    }

    /// Build a legacy transaction.
    pub fn legacy(mut self) -> Self {
        self.tx_type = TxType::Legacy;
        self
    }

    /// Build an EIP-2930 transaction.
    pub fn eip2930(mut self) -> Self {
        self.tx_type = TxType::Eip2930;
        self
    }

    /// Build an L1 priority transaction.
    pub fn l1(mut self) -> Self {
        self.tx_type = TxType::L1;
        self
    }

    /// Build an upgrade transaction.
    pub fn upgrade(mut self) -> Self {
        self.tx_type = TxType::Upgrade;
        self
    }

    /// Set chain ID (used by signed Ethereum transaction kinds).
    pub fn chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = chain_id;
        self
    }

    /// Set signer/sender wallet.
    pub fn from(mut self, signer: PrivateKeySigner) -> Self {
        self.from = Some(signer.address());
        self.signer = Some(signer);
        self
    }

    /// Set sender address directly (useful for L1/upgrade tx kinds).
    pub fn from_address(mut self, address: Address) -> Self {
        self.from = Some(address);
        self
    }

    /// Set recipient address.
    pub fn to(mut self, address: Address) -> Self {
        self.to = TxKind::Call(address);
        self
    }

    /// Mark as contract creation (`to = null`) for Ethereum transaction kinds.
    pub fn create(mut self) -> Self {
        self.to = TxKind::Create;
        self
    }

    /// Set calldata/input bytes.
    pub fn calldata(mut self, calldata: Vec<u8>) -> Self {
        self.calldata = calldata;
        self
    }

    /// Set ETH value.
    pub fn value(mut self, value: U256) -> Self {
        self.value = value;
        self
    }

    /// Set gas limit.
    pub fn gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = gas_limit;
        self
    }

    /// Set nonce.
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = nonce;
        self
    }

    /// Set max fee per gas.
    pub fn max_fee(mut self, max_fee_per_gas: u128) -> Self {
        self.max_fee_per_gas = max_fee_per_gas;
        self
    }

    /// Set max priority fee per gas.
    pub fn priority_fee(mut self, max_priority_fee_per_gas: u128) -> Self {
        self.max_priority_fee_per_gas = max_priority_fee_per_gas;
        self
    }

    /// Set EIP-2930/EIP-1559 access list.
    pub fn access_list(mut self, access_list: AccessList) -> Self {
        self.access_list = access_list;
        self
    }

    /// Set gas-per-pubdata-byte limit (L1/upgrade kinds).
    pub fn gas_per_pubdata_byte_limit(mut self, limit: u128) -> Self {
        self.gas_per_pubdata_byte_limit = limit;
        self
    }

    /// Set refund recipient (L1/upgrade kinds).
    pub fn refund_recipient(mut self, recipient: Address) -> Self {
        self.refund_recipient = Some(recipient);
        self
    }

    /// Set amount to mint on L2 (L1/upgrade kinds).
    pub fn to_mint(mut self, to_mint: U256) -> Self {
        self.to_mint = Some(to_mint);
        self
    }

    /// Set factory dependencies (L1/upgrade kinds).
    pub fn factory_deps(mut self, factory_deps: Vec<B256>) -> Self {
        self.factory_deps = factory_deps;
        self
    }

    /// Build a [`ZKsyncTxEnvelope`].
    pub fn build(self) -> ZKsyncTxEnvelope {
        match self.tx_type {
            TxType::Eip1559 => {
                let signer = self
                    .signer
                    .expect("TxBuilder: no signer set for EIP-1559 tx — call .from(signer)");
                let tx = TxEip1559 {
                    chain_id: self.chain_id,
                    nonce: self.nonce,
                    max_fee_per_gas: self.max_fee_per_gas,
                    max_priority_fee_per_gas: self.max_priority_fee_per_gas,
                    gas_limit: self.gas_limit,
                    to: self.to,
                    value: self.value,
                    access_list: self.access_list,
                    input: Bytes::from(self.calldata),
                };
                ZKsyncTxEnvelope::from_eth_tx(tx, signer)
            }
            TxType::Legacy => {
                let signer = self
                    .signer
                    .expect("TxBuilder: no signer set for legacy tx — call .from(signer)");
                let tx = TxLegacy {
                    chain_id: Some(self.chain_id),
                    nonce: self.nonce,
                    gas_price: self.max_fee_per_gas,
                    gas_limit: self.gas_limit,
                    to: self.to,
                    value: self.value,
                    input: Bytes::from(self.calldata),
                };
                ZKsyncTxEnvelope::from_eth_tx(tx, signer)
            }
            TxType::Eip2930 => {
                let signer = self
                    .signer
                    .expect("TxBuilder: no signer set for EIP-2930 tx — call .from(signer)");
                let tx = TxEip2930 {
                    chain_id: self.chain_id,
                    nonce: self.nonce,
                    gas_price: self.max_fee_per_gas,
                    gas_limit: self.gas_limit,
                    to: self.to,
                    value: self.value,
                    access_list: self.access_list,
                    input: Bytes::from(self.calldata),
                };
                ZKsyncTxEnvelope::from_eth_tx(tx, signer)
            }
            TxType::L1 => {
                let from = self.from.expect(
                    "TxBuilder: no sender set for L1 tx — call .from(signer) or .from_address(addr)",
                );
                let to = match self.to {
                    TxKind::Call(address) => address,
                    TxKind::Create => panic!("TxBuilder: L1 tx cannot be a Create"),
                };
                let to_mint = self.to_mint.unwrap_or_else(|| {
                    U256::from(self.gas_limit) * U256::from(self.max_fee_per_gas)
                });
                ZKsyncTxEnvelope::from(ZKsyncL1Tx {
                    from,
                    to,
                    gas_limit: self.gas_limit as u128,
                    gas_per_pubdata_byte_limit: self.gas_per_pubdata_byte_limit,
                    max_fee_per_gas: self.max_fee_per_gas,
                    max_priority_fee_per_gas: self.max_priority_fee_per_gas,
                    nonce: self.nonce as u128,
                    value: self.value,
                    to_mint,
                    refund_recipient: self.refund_recipient.unwrap_or_default(),
                    input: Bytes::from(self.calldata),
                    factory_deps: self.factory_deps,
                })
            }
            TxType::Upgrade => {
                let from = self.from.expect("TxBuilder: no sender set for upgrade tx — call .from(signer) or .from_address(addr)");
                let to = match self.to {
                    TxKind::Call(address) => address,
                    TxKind::Create => panic!("TxBuilder: upgrade tx cannot be a Create"),
                };
                let to_mint = self.to_mint.unwrap_or_else(|| {
                    U256::from(self.gas_limit) * U256::from(self.max_fee_per_gas)
                });
                ZKsyncTxEnvelope::from(ZKsyncUpgradeTx {
                    from,
                    to,
                    gas_limit: self.gas_limit as u128,
                    gas_per_pubdata_byte_limit: self.gas_per_pubdata_byte_limit,
                    max_fee_per_gas: self.max_fee_per_gas,
                    max_priority_fee_per_gas: self.max_priority_fee_per_gas,
                    nonce: self.nonce as u128,
                    value: self.value,
                    to_mint,
                    refund_recipient: self.refund_recipient.unwrap_or_default(),
                    input: Bytes::from(self.calldata),
                    factory_deps: self.factory_deps,
                })
            }
        }
    }
}

impl Default for TxBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::eips::Typed2718;

    #[test]
    fn l1_builder_defaults_to_computed_to_mint() {
        let tx = TxBuilder::new()
            .l1()
            .from_address(Address::ZERO)
            .to(Address::from([1u8; 20]))
            .gas_limit(10)
            .max_fee(3)
            .build();
        assert_eq!(tx.ty(), 0x7f);
    }

    #[test]
    fn upgrade_builder_sets_upgrade_type() {
        let tx = TxBuilder::new()
            .upgrade()
            .from_address(Address::ZERO)
            .to(Address::from([2u8; 20]))
            .build();
        assert_eq!(tx.ty(), 0x7e);
    }
}
