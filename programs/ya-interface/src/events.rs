//! Unified events (SPEC §4.6). Adapters emit these; the dispatcher re-emits them.
use anchor_lang::prelude::*;

#[event]
pub struct PositionInitialized {
    pub position: Pubkey,
    pub owner: Pubkey,
    pub base_mint: Pubkey,
}

#[event]
pub struct Deposited {
    pub position: Pubkey,
    pub amount: u64,
    pub value_after: u64,
}

#[event]
pub struct WithdrawRequested {
    pub position: Pubkey,
    pub shares: u64,
    pub unlock_ts: i64,
}

#[event]
pub struct WithdrawSettled {
    pub position: Pubkey,
    pub amount_out: u64,
}

#[event]
pub struct ValueReported {
    pub position: Pubkey,
    pub value: u64,
}
