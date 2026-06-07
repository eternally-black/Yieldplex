//! Shared state types. The `Position` / `WithdrawalTicket` account types are generated
//! per-adapter via [`crate::declare_ya_accounts`] so each adapter owns its PDAs while the
//! byte layout and 8-byte discriminators stay identical across all adapters.
use anchor_lang::prelude::*;

/// Lifecycle of the single withdrawal ticket per position (SPEC §4.4).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WithdrawalStatus {
    #[default]
    None,
    Pending,
    Settled,
    Cancelled,
}
