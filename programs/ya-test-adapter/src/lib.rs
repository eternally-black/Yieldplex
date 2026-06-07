#![allow(unexpected_cfgs)]
//! Throwaway validation crate (NOT a deliverable adapter). It exists only to prove that
//! `ya_interface::declare_ya_accounts!()`, the standard prefix, the `CpiCall` helper, the
//! shared events/errors, and `WithdrawalStatus` all compile cleanly inside a real Anchor
//! `#[program]`. Removed before submission.
use anchor_lang::prelude::*;
use ya_interface::{constants, cpi::CpiCall, Deposited, WithdrawalStatus, YaError};

declare_id!("So11111111111111111111111111111111111111112");

// Generates `Position` + `WithdrawalTicket` owned by THIS program, identical layout/discriminators.
ya_interface::declare_ya_accounts!();

#[program]
pub mod ya_test_adapter {
    use super::*;

    /// Exercises Position state, checked math, the uniform CPI builder, events and status.
    pub fn touch(ctx: Context<Touch>, amount: u64) -> Result<()> {
        let position = &mut ctx.accounts.position;
        position.owner = ctx.accounts.owner.key();
        position.base_mint = ctx.accounts.base_mint.key();
        position.adapter = crate::ID;
        position.shares = position.shares.checked_add(amount).ok_or(YaError::MathOverflow)?;
        position.bump = ctx.bumps.position;

        // Build (don't invoke) a CPI to prove the uniform manual-CPI primitive's ergonomics.
        let _ix = CpiCall::global(crate::ID, constants::ix::DEPOSIT)
            .arg(&amount)
            .arg(&0u64)
            .account(ctx.accounts.owner.key(), true, false)
            .instruction();

        let _status = WithdrawalStatus::Pending;
        let _ticket_space = WithdrawalTicket::SPACE;

        // exercise the current_value-as-view helper (the Solana-native view mechanism)
        ya_interface::report_value(position.shares);

        emit!(Deposited {
            position: position.key(),
            amount,
            value_after: position.shares,
        });
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Touch<'info> {
    #[account(
        init_if_needed,
        payer = owner,
        space = Position::SPACE,
        seeds = [Position::SEED, owner.key().as_ref(), base_mint.key().as_ref()],
        bump,
    )]
    pub position: Account<'info, Position>,
    /// CHECK: used only as a seed; not read or written here.
    pub base_mint: UncheckedAccount<'info>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}
