use std::alloc::Global;

use basic_bootloader::bootloader::transaction_flow::zk::ZkTransactionFlowOnlyEOA;
use basic_bootloader::bootloader::BasicBootloader;
use basic_system::system_functions::NoStdSystemFunctions;
use basic_system::system_implementation::system::EthereumLikeStorageAccessCostModel;
use basic_system::system_implementation::system::FullIO;
use oracle_provider::DummyMemorySource;
use oracle_provider::ZkEENonDeterminismSource;
use zk_ee::memory::stack_implementations::vec_stack::VecStackFactory;
use zk_ee::oracle::IOOracle;
use zk_ee::reference_implementations::BaseResources;
use zk_ee::system::{EthereumLikeTypes, SystemTypes};
use zk_ee::types_config::EthereumIOTypesConfig;

#[cfg(not(feature = "no_print"))]
type Logger = crate::system::logger::StdIOLogger;

#[cfg(feature = "no_print")]
type Logger = zk_ee::system::NullLogger;

pub struct ForwardSystemTypes<O, const PROOF_ENV: bool>(O);

type Native = zk_ee::reference_implementations::DecreasingNative;

impl<O: IOOracle, const PROOF_ENV: bool> SystemTypes for ForwardSystemTypes<O, PROOF_ENV> {
    type IOTypes = EthereumIOTypesConfig;
    type Resources = BaseResources<Native>;
    type IO = FullIO<
        Self::Allocator,
        Self::Resources,
        EthereumLikeStorageAccessCostModel,
        VecStackFactory,
        0,
        O,
        PROOF_ENV,
    >;
    // For PROOF_ENV=true, we can enable delegation-based modexp to capture prover input.
    type SystemFunctions = NoStdSystemFunctions<PROOF_ENV>;
    type SystemFunctionsExt = NoStdSystemFunctions<PROOF_ENV>;
    type Allocator = Global;
    type Logger = Logger;
    type Metadata = zk_ee::system::metadata::zk_metadata::ZkMetadata;
}

impl<O: IOOracle, const PROOF_ENV: bool> EthereumLikeTypes for ForwardSystemTypes<O, PROOF_ENV> {}

pub type ForwardRunningSystem =
    ForwardSystemTypes<ZkEENonDeterminismSource<DummyMemorySource>, false>;

pub type CallSimulationSystem =
    ForwardSystemTypes<ZkEENonDeterminismSource<DummyMemorySource>, false>;

pub type ProverInputSystem =
    ForwardSystemTypes<oracle_provider::ReadWitnessSource<DummyMemorySource>, true>;

pub type ForwardBootloader = BasicBootloader<ForwardRunningSystem, ZkTransactionFlowOnlyEOA>;

pub type CallSimulationBootloader = BasicBootloader<CallSimulationSystem, ZkTransactionFlowOnlyEOA>;

pub type ProverInputBootloader = BasicBootloader<ProverInputSystem, ZkTransactionFlowOnlyEOA>;
