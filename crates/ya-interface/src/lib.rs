#![allow(unexpected_cfgs)]
//! # Yield Adapter Standard — interface crate
//!
//! This crate IS the standard. It has no entrypoint. Adapters, the dispatcher, and the
//! registry depend on it. It defines: the standard instruction names + discriminators,
//! the account prefix, the `Position`/`WithdrawalTicket` account layouts (generated per
//! adapter via [`declare_ya_accounts`]), shared errors and events, and the uniform
//! manual-CPI helper ([`cpi::CpiCall`]) used by every adapter.

pub mod constants;
pub mod cpi;
pub mod error;
pub mod events;
pub mod view;

pub use constants::*;
pub use cpi::{anchor_discriminator, CpiCall};
pub use error::YaError;
pub use events::*;
pub use view::{read_returned_value, report_value};

/// Generate the standard `Position` and `WithdrawalTicket` Anchor account types **inside the
/// calling adapter crate**. Because the macro expands in the adapter, each account is owned by
/// that adapter's program id, yet the field layout and 8-byte discriminators are identical
/// across every adapter (so one SDK decoder works for all of them).
///
/// Usage (in an adapter `lib.rs`, after `declare_id!`):
/// ```ignore
/// ya_interface::declare_ya_accounts!();
/// ```
#[macro_export]
macro_rules! declare_ya_accounts {
    () => {
        /// Lifecycle of the single withdrawal ticket per position (SPEC §4.4). Generated locally
        /// in each adapter (identical layout/discriminator everywhere) so adapter IDLs stay
        /// self-contained (no cross-crate IdlBuild).
        #[derive(
            anchor_lang::AnchorSerialize,
            anchor_lang::AnchorDeserialize,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Debug,
            Default,
        )]
        pub enum WithdrawalStatus {
            #[default]
            None,
            Pending,
            Settled,
            Cancelled,
        }

        /// Per-(owner, base_mint) position record. PDA seeds `[b"position", owner, base_mint]`.
        #[account]
        #[derive(Default)]
        pub struct Position {
            /// End user who owns this position.
            pub owner: Pubkey,
            /// Base asset mint (USDC).
            pub base_mint: Pubkey,
            /// Adapter program that owns this position.
            pub adapter: Pubkey,
            /// Protocol-native position units held (cTokens / shares / JLP / if_shares / syrup).
            pub shares: u64,
            /// Last computed redeemable base-asset value (cached by `current_value`).
            pub cached_value: u64,
            /// Unix seconds when `cached_value` was last updated.
            pub value_updated_ts: i64,
            /// Canonical bump for this position PDA.
            pub bump: u8,
            /// Canonical bump for the vault_authority PDA `[b"vault_authority", position]`.
            pub vault_authority_bump: u8,
        }

        impl Position {
            pub const SEED: &'static [u8] = $crate::constants::seeds::POSITION;
            /// 8 disc + owner + base_mint + adapter + shares + cached_value + value_updated_ts + 2 bumps.
            pub const SPACE: usize = 8 + 32 + 32 + 32 + 8 + 8 + 8 + 1 + 1;
        }

        /// The single active withdrawal ticket per position. PDA seeds `[b"ticket", position]`.
        #[account]
        #[derive(Default)]
        pub struct WithdrawalTicket {
            /// The position this ticket belongs to.
            pub position: Pubkey,
            /// Position units being withdrawn.
            pub shares: u64,
            /// Minimum acceptable base-asset payout (slippage protection).
            pub min_amount_out: u64,
            /// 0 (or <= now) means settle-now; otherwise the cooldown unlock time (unix seconds).
            pub unlock_ts: i64,
            /// Ticket lifecycle status.
            pub status: WithdrawalStatus,
            /// Unix seconds when the ticket was created.
            pub created_ts: i64,
            /// Canonical bump for this ticket PDA.
            pub bump: u8,
        }

        impl WithdrawalTicket {
            pub const SEED: &'static [u8] = $crate::constants::seeds::TICKET;
            /// 8 disc + position + shares + min_amount_out + unlock_ts + status(1) + created_ts + bump.
            pub const SPACE: usize = 8 + 32 + 8 + 8 + 8 + 1 + 8 + 1;
        }
    };
}
