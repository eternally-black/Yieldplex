// ============================================================
// Program:  ya-cooldown-standin  (Yield Adapter Standard — two-phase conformance STAND-IN)
// Framework: Anchor 1.0.2
// Risk Level: 🟢 Low — no real protocol, mock 1:1 shares, no token movement.
//
// THIS IS NOT A LIVE PROTOCOL ADAPTER. It is the honest Drift stand-in for the Drift Insurance Fund
// adapter: a real on-chain CPI round-trip into Drift's IF-staking is impossible for any integration
// (those instructions are commented out of Drift's deployed #[program] — see ya-adapter-drift-if +
// docs/adapters/drift-if.md). To still prove that OUR two-phase machinery is correct — the standard
// request->cooldown->settle withdrawal and the dispatcher's two-phase routing — we run the
// conformance suite + a full lifecycle (deposit -> request -> time-travel -> settle) against this
// minimal cooldown stand-in. It is labelled a stand-in everywhere and is NEVER presented as a live
// Drift pass (RESULTS.md). The cooldown shape mirrors Drift's IF unstaking period.
// ============================================================
#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use ya_interface::{
    constants::seeds, report_value, Deposited, PositionInitialized, ValueReported, WithdrawRequested,
    WithdrawSettled, YaError,
};
use ya_registry::AdapterStatus;

declare_id!("7aTuXKiyKwZ1MVrPfMgoAPoh8VKDSBLPAfEBvzkhJCYR");

ya_interface::declare_ya_accounts!();

/// Cooldown (seconds) — mirrors Drift's IF unstaking period (13 days) so the two-phase shape is realistic.
const COOLDOWN_SECONDS: i64 = 1_123_200;

#[program]
pub mod ya_cooldown_standin {
    use super::*;

    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> {
        let p = &mut ctx.accounts.position;
        if p.owner == Pubkey::default() {
            p.owner = ctx.accounts.owner.key();
            p.base_mint = ctx.accounts.base_mint.key();
            p.adapter = crate::ID;
            p.shares = 0;
            p.bump = ctx.bumps.position;
            p.vault_authority_bump = ctx.bumps.vault_authority;
            emit!(PositionInitialized { position: p.key(), owner: p.owner, base_mint: p.base_mint });
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
        emit!(Deposited { position: p.key(), amount, value_after: p.cached_value });
        Ok(())
    }

    /// Two-phase: opens a Pending ticket with a future unlock; shares stay until settle.
    pub fn withdraw(ctx: Context<WithdrawOp>, shares: u64, min_amount_out: u64) -> Result<()> {
        assert_active(&ctx.accounts.registry_entry.to_account_info(), &ctx.accounts.base_mint.key())?;
        let p = &ctx.accounts.position;
        require!(shares <= p.shares, YaError::SlippageExceeded);
        let now = Clock::get()?.unix_timestamp;
        let unlock = now.checked_add(COOLDOWN_SECONDS).ok_or(YaError::MathOverflow)?;
        let t = &mut ctx.accounts.ticket;
        require!(t.status != WithdrawalStatus::Pending, YaError::TicketAlreadyExists);
        t.position = p.key();
        t.shares = shares;
        t.min_amount_out = min_amount_out;
        t.unlock_ts = unlock;
        t.status = WithdrawalStatus::Pending;
        t.created_ts = now;
        t.bump = ctx.bumps.ticket;
        emit!(WithdrawRequested { position: p.key(), shares, unlock_ts: unlock });
        Ok(())
    }

    /// Settle a Pending ticket after its cooldown unlock; reduces shares (mock 1:1 payout).
    pub fn settle_withdrawal(ctx: Context<WithdrawOp>, min_amount_out: u64) -> Result<()> {
        assert_active(&ctx.accounts.registry_entry.to_account_info(), &ctx.accounts.base_mint.key())?;
        let now = Clock::get()?.unix_timestamp;
        let (shares, unlock) = {
            let t = &ctx.accounts.ticket;
            require!(t.status == WithdrawalStatus::Pending, YaError::NothingToSettle);
            (t.shares, t.unlock_ts)
        };
        require!(now >= unlock, YaError::WithdrawalLocked);
        let amount_out = shares; // mock 1:1
        require!(amount_out >= min_amount_out, YaError::SlippageExceeded);
        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_sub(shares).ok_or(YaError::MathOverflow)?;
        p.cached_value = p.shares;
        p.value_updated_ts = now;
        let t = &mut ctx.accounts.ticket;
        t.status = WithdrawalStatus::Settled;
        emit!(WithdrawSettled { position: p.key(), amount_out });
        Ok(())
    }

    pub fn current_value(ctx: Context<Op>) -> Result<()> {
        assert_active(&ctx.accounts.registry_entry.to_account_info(), &ctx.accounts.base_mint.key())?;
        let p = &mut ctx.accounts.position;
        let value = p.shares; // mock 1:1
        p.cached_value = value;
        p.value_updated_ts = Clock::get()?.unix_timestamp;
        report_value(value);
        emit!(ValueReported { position: p.key(), value });
        Ok(())
    }
}

fn assert_active(registry_entry: &AccountInfo, base_mint: &Pubkey) -> Result<()> {
    let entry = ya_registry::load_adapter_entry(registry_entry, &crate::ID)?;
    require!(entry.status == AdapterStatus::Active, YaError::AdapterNotActive);
    require_keys_eq!(entry.base_mint, *base_mint, YaError::BaseMintMismatch);
    Ok(())
}

#[derive(Accounts)]
pub struct InitializePosition<'info> {
    #[account(init_if_needed, payer = owner, space = Position::SPACE,
        seeds = [seeds::POSITION, owner.key().as_ref(), base_mint.key().as_ref()], bump)]
    pub position: Account<'info, Position>,
    /// CHECK: vault_authority PDA.
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump)]
    pub vault_authority: UncheckedAccount<'info>,
    /// CHECK: base mint (seed only).
    pub base_mint: UncheckedAccount<'info>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Op<'info> {
    #[account(mut, seeds = [seeds::POSITION, owner.key().as_ref(), base_mint.key().as_ref()],
        bump = position.bump, has_one = owner, has_one = base_mint)]
    pub position: Account<'info, Position>,
    /// CHECK: vault_authority PDA.
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump = position.vault_authority_bump)]
    pub vault_authority: UncheckedAccount<'info>,
    /// CHECK: base mint.
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: unused by the stand-in.
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    pub owner: Signer<'info>,
    /// CHECK: unused by the stand-in.
    #[account(mut)]
    pub owner_token_account: UncheckedAccount<'info>,
    /// CHECK: validated in assert_active.
    pub registry_entry: UncheckedAccount<'info>,
    /// CHECK: unused.
    pub token_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawOp<'info> {
    #[account(mut, seeds = [seeds::POSITION, owner.key().as_ref(), base_mint.key().as_ref()],
        bump = position.bump, has_one = owner, has_one = base_mint)]
    pub position: Account<'info, Position>,
    /// CHECK: vault_authority PDA.
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump = position.vault_authority_bump)]
    pub vault_authority: UncheckedAccount<'info>,
    /// CHECK: base mint.
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: unused by the stand-in.
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: unused by the stand-in.
    #[account(mut)]
    pub owner_token_account: UncheckedAccount<'info>,
    /// CHECK: validated in assert_active.
    pub registry_entry: UncheckedAccount<'info>,
    /// CHECK: unused.
    pub token_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    #[account(init_if_needed, payer = owner, space = WithdrawalTicket::SPACE,
        seeds = [seeds::TICKET, position.key().as_ref()], bump)]
    pub ticket: Account<'info, WithdrawalTicket>,
}
