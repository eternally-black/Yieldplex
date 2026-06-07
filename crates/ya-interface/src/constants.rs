//! Shared constants of the Yield Adapter Standard. Identical across every adapter.
use anchor_lang::prelude::*;

/// Base asset of the reference build: USDC (6 decimals).
pub mod usdc {
    use anchor_lang::prelude::*;
    pub const MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    pub const DECIMALS: u8 = 6;
}

/// PDA seed prefixes. Each adapter owns its own PDAs but uses these identical seeds.
pub mod seeds {
    /// `[POSITION, owner, base_mint]`
    pub const POSITION: &[u8] = b"position";
    /// `[VAULT_AUTHORITY, position]` — signs all protocol CPIs, owns vault token accounts + sub-accounts.
    pub const VAULT_AUTHORITY: &[u8] = b"vault_authority";
    /// `[TICKET, position]` — one active withdrawal ticket per position.
    pub const TICKET: &[u8] = b"ticket";
}

/// Standard instruction names. Discriminators derive as sha256("global:<name>")[..8],
/// so they are identical across all adapters (see [`crate::cpi::anchor_discriminator`]).
pub mod ix {
    pub const INITIALIZE_POSITION: &str = "initialize_position";
    pub const DEPOSIT: &str = "deposit";
    pub const WITHDRAW: &str = "withdraw";
    pub const SETTLE_WITHDRAWAL: &str = "settle_withdrawal";
    pub const CURRENT_VALUE: &str = "current_value";
    pub const CANCEL_WITHDRAWAL: &str = "cancel_withdrawal";
}

/// The fixed standard account prefix length (indices 0..=8). Protocol-specific accounts
/// follow as `remaining_accounts`. See SPEC §4.3 for the exact order/mutability:
///   0 position(w) 1 vault_authority 2 base_mint 3 vault_token_account(w)
///   4 owner(signer) 5 owner_token_account(w) 6 registry_entry 7 token_program 8 system_program
pub const PREFIX_LEN: usize = 9;
