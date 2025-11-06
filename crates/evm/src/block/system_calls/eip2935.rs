//! [EIP-2935](https://eips.ethereum.org/EIPS/eip-2935) system call implementation.

use crate::{
    block::{BlockExecutionError, BlockValidationError},
    Evm,
};
use alloc::{format, string::ToString};
use alloy_eips::eip2935::HISTORY_STORAGE_ADDRESS;
use alloy_hardforks::EthereumHardforks;
use alloy_primitives::B256;
use revm::{context::Block, context_interface::result::ResultAndState};
use tracing::info;

/// Applies the pre-block call to the [EIP-2935] blockhashes contract, using the given block,
/// chain specification, and EVM.
///
/// If Prague is not activated, or the block is the genesis block, then this is a no-op, and no
/// state changes are made.
///
/// Note: this does not commit the state changes to the database, it only transact the call.
///
/// Returns `None` if Prague is not active or the block is the genesis block, otherwise returns the
/// result of the call.
///
/// [EIP-2935]: https://eips.ethereum.org/EIPS/eip-2935
#[inline]
pub(crate) fn transact_blockhashes_contract_call<Halt>(
    spec: impl EthereumHardforks,
    parent_block_hash: B256,
    evm: &mut impl Evm<HaltReason = Halt>,
) -> Result<Option<ResultAndState<Halt>>, BlockExecutionError> {
    if !spec.is_prague_active_at_timestamp(evm.block().timestamp().saturating_to()) {
        return Ok(None);
    }

    // if the block number is zero (genesis block) then no system transaction may occur as per
    // EIP-2935
    if evm.block().number().is_zero() {
        return Ok(None);
    }

    info!("transacting block hashes contract call");
    info!("system address: {}", alloy_eips::eip4788::SYSTEM_ADDRESS);
    info!("history storage address: {}", HISTORY_STORAGE_ADDRESS);
    info!("parent block hash: {}", parent_block_hash);

    // Check if the HISTORY_STORAGE_ADDRESS has code by attempting to read it
    // Note: We can't directly check the database without changing the function signature,
    // so we rely on the system call to fail with a clear error if the address has no code.
    // The check is performed implicitly when transact_system_call is executed.
    let res = match evm.transact_system_call(
        alloy_eips::eip4788::SYSTEM_ADDRESS,
        HISTORY_STORAGE_ADDRESS,
        parent_block_hash.0.into(),
    ) {
        Ok(res) => res,
        Err(e) => {
            let error_msg = e.to_string();
            // Check if the error indicates the address has no code or revert without message
            // The contract reverts with no message (0x) in @throw when:
            // - calldatasize != 32 (read mode)
            // - input > number - 1 (read mode)
            // - number - input > BUFLEN (read mode)
            // Also, if the contract code doesn't exist, EVM will fail
            if error_msg.contains("no code") 
                || error_msg.contains("does not exist")
                || error_msg.contains("revert")
                || error_msg.contains("0x") {
                return Err(BlockValidationError::BlockHashContractCall {
                    message: format!("history storage address call failed: {}", error_msg),
                }.into());
            }
            return Err(
                BlockValidationError::BlockHashContractCall { message: error_msg }.into()
            )
        }
    };

    Ok(Some(res))
}
