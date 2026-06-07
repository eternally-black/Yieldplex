//! Shared error taxonomy for the Yield Adapter Standard (SPEC §4.6).
use anchor_lang::prelude::*;

#[error_code]
pub enum YaError {
    #[msg("Adapter is not Active in the registry")]
    AdapterNotActive,
    #[msg("Base mint does not match the registry/adapter base mint")]
    BaseMintMismatch,
    #[msg("Slippage exceeded: output below min_amount_out / position below min_position_out")]
    SlippageExceeded,
    #[msg("Withdrawal is still locked (now < unlock_ts)")]
    WithdrawalLocked,
    #[msg("Nothing to settle: no pending withdrawal ticket")]
    NothingToSettle,
    #[msg("A withdrawal ticket already exists for this position")]
    TicketAlreadyExists,
    #[msg("Oracle/price source is stale or unavailable; failing closed")]
    OracleStale,
    #[msg("Invalid remaining accounts (count/owner/type/order mismatch)")]
    InvalidRemainingAccounts,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Registry program id / adapter program id mismatch")]
    AdapterProgramMismatch,
}
