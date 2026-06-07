// ============================================================
// Program:  ya-mock-adapter  (Yield Adapter Standard — reference/mock adapter)
// Framework: Anchor 1.0.2
// Risk Level: 🟢 Low — no real protocol, no token movement; exercises the standard interface.
//
// A minimal, instant adapter that implements the full standard interface (§4.2) with the
// standard prefix (§4.3). It moves no tokens (mock 1:1 shares) so it can validate the dispatcher
// route, the conformance suite, the Position/WithdrawalTicket macro, and the return-data view —
// independent of any external program. Real adapters (M5) follow this exact shape, swapping the
// mock body for one `CpiCall` into the protocol.
// ============================================================
#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use ya_interface::{
    constants::seeds, report_value, Deposited, PositionInitialized, ValueReported, WithdrawSettled,
    YaError,
};
use ya_registry::AdapterStatus;

declare_id!("kQJWqDnHj7mSugETc3EmFrgdwGRoEVSRuVDGgM9ZXjK");

ya_interface::declare_ya_accounts!(); // Position + WithdrawalTicket (owned by this program)

#[program]
pub mod ya_mock_adapter {
    use super::*;

    /// Idempotent. Creates the Position (and would create protocol sub-accounts in a real adapter).
    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> {
        let p = &mut ctx.accounts.position;
        if p.owner == Pubkey::default() {
            p.owner = ctx.accounts.owner.key();
            p.base_mint = ctx.accounts.base_mint.key();
            p.adapter = crate::ID;
            p.shares = 0;
            p.bump = ctx.bumps.position;
            p.vault_authority_bump = ctx.bumps.vault_authority;
            emit!(PositionInitialized {
                position: p.key(),
                owner: p.owner,
                base_mint: p.base_mint,
            });
        }
        Ok(())
    }

    pub fn deposit(ctx: Context<Op>, amount: u64, min_position_out: u64) -> Result<()> {
        assert_active(&ctx.accounts.registry_entry.to_account_info(), &ctx.accounts.base_mint.key())?;
        let out = amount; // mock 1:1
        require!(out >= min_position_out, YaError::SlippageExceeded);
        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_add(out).ok_or(YaError::MathOverflow)?;
        p.cached_value = p.shares;
        p.value_updated_ts = Clock::get()?.unix_timestamp;
        emit!(Deposited {
            position: p.key(),
            amount,
            value_after: p.cached_value,
        });
        Ok(())
    }

    /// Instant withdraw: settles in the same call, writing a Settled ticket.
    pub fn withdraw(ctx: Context<WithdrawOp>, shares: u64, min_amount_out: u64) -> Result<()> {
        assert_active(&ctx.accounts.registry_entry.to_account_info(), &ctx.accounts.base_mint.key())?;
        let p = &mut ctx.accounts.position;
        require!(shares <= p.shares, YaError::SlippageExceeded);
        let amount_out = shares; // mock 1:1
        require!(amount_out >= min_amount_out, YaError::SlippageExceeded);
        p.shares = p.shares.checked_sub(shares).ok_or(YaError::MathOverflow)?;
        p.cached_value = p.shares;
        p.value_updated_ts = Clock::get()?.unix_timestamp;

        let t = &mut ctx.accounts.ticket;
        t.position = p.key();
        t.shares = shares;
        t.min_amount_out = min_amount_out;
        t.unlock_ts = 0;
        t.status = WithdrawalStatus::Settled;
        t.created_ts = Clock::get()?.unix_timestamp;
        t.bump = ctx.bumps.ticket;

        emit!(WithdrawSettled {
            position: p.key(),
            amount_out,
        });
        Ok(())
    }

    /// Instant adapter: nothing to settle (withdraw already settled).
    pub fn settle_withdrawal(_ctx: Context<Op>, _min_amount_out: u64) -> Result<()> {
        Err(YaError::NothingToSettle.into())
    }

    /// View: report redeemable base-asset value via return data (+ cache it).
    pub fn current_value(ctx: Context<Op>) -> Result<()> {
        assert_active(&ctx.accounts.registry_entry.to_account_info(), &ctx.accounts.base_mint.key())?;
        let p = &mut ctx.accounts.position;
        let value = p.shares; // mock 1:1
        p.cached_value = value;
        p.value_updated_ts = Clock::get()?.unix_timestamp;
        report_value(value);
        emit!(ValueReported {
            position: p.key(),
            value,
        });
        Ok(())
    }
}

/// Adapters assert the registry entry themselves so direct (depth-1) calls are gated too (§7).
/// IDL-free load (owner + discriminator + canonical PDA + program_id == this adapter).
fn assert_active(registry_entry: &AccountInfo, base_mint: &Pubkey) -> Result<()> {
    let entry = ya_registry::load_adapter_entry(registry_entry, &crate::ID)?;
    require!(entry.status == AdapterStatus::Active, YaError::AdapterNotActive);
    require_keys_eq!(entry.base_mint, *base_mint, YaError::BaseMintMismatch);
    Ok(())
}

// ── accounts ────────────────────────────────────────────────
#[derive(Accounts)]
pub struct InitializePosition<'info> {
    #[account(
        init_if_needed,
        payer = owner,
        space = Position::SPACE,
        seeds = [seeds::POSITION, owner.key().as_ref(), base_mint.key().as_ref()],
        bump,
    )]
    pub position: Account<'info, Position>,
    /// CHECK: vault_authority PDA (signs protocol CPIs in real adapters); no data here.
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump)]
    pub vault_authority: UncheckedAccount<'info>,
    /// CHECK: base mint (seed only).
    pub base_mint: UncheckedAccount<'info>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

/// The standard prefix (§4.3) — identical for deposit / settle_withdrawal / current_value.
#[derive(Accounts)]
pub struct Op<'info> {
    #[account(
        mut,
        seeds = [seeds::POSITION, owner.key().as_ref(), base_mint.key().as_ref()],
        bump = position.bump,
        has_one = owner,
        has_one = base_mint,
    )]
    pub position: Account<'info, Position>,
    /// CHECK: vault_authority PDA.
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump = position.vault_authority_bump)]
    pub vault_authority: UncheckedAccount<'info>,
    /// CHECK: base mint.
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: vault token account (unused by the mock).
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    pub owner: Signer<'info>,
    /// CHECK: owner token account (unused by the mock).
    #[account(mut)]
    pub owner_token_account: UncheckedAccount<'info>,
    /// CHECK: validated in assert_active via ya_registry::load_adapter_entry (owner+disc+PDA+program_id).
    pub registry_entry: UncheckedAccount<'info>,
    /// CHECK: token program (unused by the mock).
    pub token_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// Withdraw adds the standard withdrawal ticket (passed as the first remaining account by the
/// dispatcher) on top of the prefix.
#[derive(Accounts)]
pub struct WithdrawOp<'info> {
    #[account(
        mut,
        seeds = [seeds::POSITION, owner.key().as_ref(), base_mint.key().as_ref()],
        bump = position.bump,
        has_one = owner,
        has_one = base_mint,
    )]
    pub position: Account<'info, Position>,
    /// CHECK: vault_authority PDA.
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump = position.vault_authority_bump)]
    pub vault_authority: UncheckedAccount<'info>,
    /// CHECK: base mint.
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: vault token account (unused by the mock).
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: owner token account (unused by the mock).
    #[account(mut)]
    pub owner_token_account: UncheckedAccount<'info>,
    /// CHECK: validated in assert_active via ya_registry::load_adapter_entry (owner+disc+PDA+program_id).
    pub registry_entry: UncheckedAccount<'info>,
    /// CHECK: token program (unused by the mock).
    pub token_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    #[account(
        init_if_needed,
        payer = owner,
        space = WithdrawalTicket::SPACE,
        seeds = [seeds::TICKET, position.key().as_ref()],
        bump,
    )]
    pub ticket: Account<'info, WithdrawalTicket>,
}
