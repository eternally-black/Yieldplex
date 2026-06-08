// ============================================================
// Program:  ya-adapter-drift-if  (Yield Adapter Standard — Drift v2 Insurance Fund USDC adapter)
// Framework: Anchor 1.0.2
// Risk Level: 🔴 Critical — two-phase (cooldown) IF staking via the per-position vault_authority PDA.
// Security:  See programs/ya-adapter-drift-if/security-checklist.md
//
// Reference adapter #5 — the TWO-PHASE adapter (request -> cooldown -> settle), and the honest §F
// play. A live IF-staking CPI is IMPOSSIBLE for ANY submission: Drift's deployed program has the
// *_insurance_fund_stake instructions commented out of its #[program] (see docs/adapters/drift-if.md
// + `yarn probe:drift-if`). So this adapter is SPEC-CORRECT (real Drift accounts/discriminators,
// two-phase ticket, cooldown read from chain, IF-share value math) — it executes the instant Drift
// re-enables the exports — and its two-phase machinery is proven green on ya-cooldown-standin. It is
// never presented as a live Drift pass.
//
//   initialize_position -> Position + vault USDC + CPI initialize_user_stats + initialize_insurance_fund_stake
//   deposit  -> transfer owner USDC -> vault, then add_insurance_fund_stake(market_index, amount)
//   withdraw -> request_remove_insurance_fund_stake(market_index, shares); ticket Pending,
//               unlock_ts = now + SpotMarket.insurance_fund.unstaking_period (read from chain)
//   settle_withdrawal -> after unlock: remove_insurance_fund_stake(market_index) -> vault -> owner
//   current_value -> if_shares * IF_vault_balance / total_if_shares (oracle-free), capped at
//                    last_withdraw_request_value once a withdrawal is pending (§11).
// ============================================================
#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::{invoke, invoke_signed},
};
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
use ya_interface::{
    constants::seeds, report_value, CpiCall, Deposited, PositionInitialized, ValueReported,
    WithdrawRequested, WithdrawSettled, YaError,
};
use ya_registry::AdapterStatus;

declare_id!("8MYJzh7Fm1q6QcrXNZNvCetoLkv1tfxjBDbrZXTFVjLs");

ya_interface::declare_ya_accounts!();

const DRIFT_ID: Pubkey = pubkey!("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH");
const MARKET_INDEX: u16 = 0; // USDC spot market
const SPOT_MARKET_DISC: [u8; 8] = [100, 177, 8, 107, 168, 65, 65, 39];
const IF_STAKE_DISC: [u8; 8] = [110, 202, 14, 42, 95, 73, 90, 95];
// SpotMarket offsets: insurance_fund.vault@304, total_shares(u128)@336, unstaking_period(i64)@384.
const SM_IF_VAULT: usize = 304;
const SM_TOTAL_SHARES: usize = 336;
const SM_UNSTAKING_PERIOD: usize = 384;
// InsuranceFundStake offsets: if_shares(u128)@40, last_withdraw_request_shares(u128)@56,
// last_withdraw_request_value(u64)@96.
const IFS_IF_SHARES: usize = 40;
const IFS_LAST_REQ_SHARES: usize = 56;
const IFS_LAST_REQ_VALUE: usize = 96;

const VAULT_USDC_SEED: &[u8] = b"vault_usdc";

#[program]
pub mod ya_adapter_drift_if {
    use super::*;

    /// Idempotent. Position + vault USDC + (once) the Drift UserStats and InsuranceFundStake PDAs.
    /// remaining: 0 user_stats(w) 1 if_stake(w) 2 state(w) 3 spot_market 4 rent 5 drift_program
    pub fn initialize_position<'info>(ctx: Context<'info, InitializePosition<'info>>) -> Result<()> {
        let p = &mut ctx.accounts.position;
        let fresh = p.owner == Pubkey::default();
        if fresh {
            p.owner = ctx.accounts.owner.key();
            p.base_mint = ctx.accounts.base_mint.key();
            p.adapter = crate::ID;
            p.shares = 0;
            p.bump = ctx.bumps.position;
            p.vault_authority_bump = ctx.bumps.vault_authority;
            emit!(PositionInitialized { position: p.key(), owner: p.owner, base_mint: p.base_mint });
        }
        if fresh {
            let ra = ctx.remaining_accounts;
            require!(ra.len() >= 6, YaError::InvalidRemainingAccounts);
            require_keys_eq!(ra[5].key(), DRIFT_ID, YaError::InvalidRemainingAccounts);
            let (va, vb) = (ctx.accounts.vault_authority.to_account_info(), ctx.accounts.owner.to_account_info());
            let pos = ctx.accounts.position.key();
            let bump = [ctx.accounts.position.vault_authority_bump];
            let signer: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, pos.as_ref(), &bump]];
            // initialize_user_stats: user_stats(w) state(w) authority payer(w,signer) rent system_program
            CpiCall::global(DRIFT_ID, "initialize_user_stats")
                .account(ra[0].key(), false, true).account(ra[2].key(), false, true)
                .account(va.key(), true, false).account(vb.key(), true, true)
                .account(ra[4].key(), false, false).account(ctx.accounts.system_program.key(), false, false)
                .invoke_signed(&[ra[0].clone(), ra[2].clone(), va.clone(), vb.clone(), ra[4].clone(),
                    ctx.accounts.system_program.to_account_info(), ra[5].clone()], signer)?;
            // initialize_insurance_fund_stake(market_index): spot_market if_stake(w) user_stats(w) state authority(signer) payer(w,signer) rent system_program
            CpiCall::global(DRIFT_ID, "initialize_insurance_fund_stake").arg(&MARKET_INDEX)
                .account(ra[3].key(), false, false).account(ra[1].key(), false, true).account(ra[0].key(), false, true)
                .account(ra[2].key(), false, false).account(va.key(), true, false).account(vb.key(), true, true)
                .account(ra[4].key(), false, false).account(ctx.accounts.system_program.key(), false, false)
                .invoke_signed(&[ra[3].clone(), ra[1].clone(), ra[0].clone(), ra[2].clone(), va.clone(), vb.clone(),
                    ra[4].clone(), ctx.accounts.system_program.to_account_info(), ra[5].clone()], signer)?;
        }
        Ok(())
    }

    /// remaining: 0 state 1 spot_market(w) 2 if_stake(w) 3 user_stats(w) 4 spot_market_vault(w)
    ///            5 if_vault(w) 6 drift_signer 7 drift_program
    pub fn deposit<'info>(ctx: Context<'info, Op<'info>>, amount: u64, min_position_out: u64) -> Result<()> {
        assert_active(&ctx.accounts.registry_entry.to_account_info(), &ctx.accounts.base_mint.key())?;
        require!(amount >= min_position_out, YaError::SlippageExceeded);
        let ra = ctx.remaining_accounts;
        require!(ra.len() >= 8, YaError::InvalidRemainingAccounts);
        validate_drift(&ra[1], SPOT_MARKET_DISC)?;
        require_keys_eq!(ra[7].key(), DRIFT_ID, YaError::InvalidRemainingAccounts);
        let decimals = mint_decimals(&ctx.accounts.base_mint.to_account_info())?;
        spl_transfer_checked(&ctx.accounts.token_program.to_account_info(),
            &ctx.accounts.owner_token_account.to_account_info(), &ctx.accounts.base_mint.to_account_info(),
            &ctx.accounts.vault_token_account.to_account_info(), &ctx.accounts.owner.to_account_info(),
            amount, decimals, None)?;
        let pos = ctx.accounts.position.key();
        let bump = [ctx.accounts.position.vault_authority_bump];
        let sd: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, pos.as_ref(), &bump]];
        // add_insurance_fund_stake(market_index, amount): state spot_market(w) if_stake(w) user_stats(w)
        //   authority(signer) spot_market_vault(w) if_vault(w) drift_signer user_token_account(w) token_program
        CpiCall::global(DRIFT_ID, "add_insurance_fund_stake").arg(&MARKET_INDEX).arg(&amount)
            .account(ra[0].key(), false, false).account(ra[1].key(), false, true).account(ra[2].key(), false, true)
            .account(ra[3].key(), false, true).account(ctx.accounts.vault_authority.key(), true, false)
            .account(ra[4].key(), false, true).account(ra[5].key(), false, true).account(ra[6].key(), false, false)
            .account(ctx.accounts.vault_token_account.key(), false, true).account(ctx.accounts.token_program.key(), false, false)
            .invoke_signed(&[ra[0].clone(), ra[1].clone(), ra[2].clone(), ra[3].clone(),
                ctx.accounts.vault_authority.to_account_info(), ra[4].clone(), ra[5].clone(), ra[6].clone(),
                ctx.accounts.vault_token_account.to_account_info(), ctx.accounts.token_program.to_account_info(), ra[7].clone()], sd)?;
        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_add(amount).ok_or(YaError::MathOverflow)?;
        p.value_updated_ts = Clock::get()?.unix_timestamp;
        emit!(Deposited { position: p.key(), amount, value_after: p.shares });
        Ok(())
    }

    /// Phase 1: request unstake. remaining: 0 spot_market(w) 1 if_stake(w) 2 user_stats(w) 3 if_vault(w) 4 drift_program
    pub fn withdraw<'info>(ctx: Context<'info, WithdrawOp<'info>>, shares: u64, min_amount_out: u64) -> Result<()> {
        assert_active(&ctx.accounts.registry_entry.to_account_info(), &ctx.accounts.base_mint.key())?;
        let ra = ctx.remaining_accounts;
        require!(ra.len() >= 5, YaError::InvalidRemainingAccounts);
        require!(shares <= ctx.accounts.position.shares, YaError::SlippageExceeded);
        let unstaking_period = read_i64(&ra[0], SPOT_MARKET_DISC, SM_UNSTAKING_PERIOD)?; // from chain (§C4)
        require_keys_eq!(ra[4].key(), DRIFT_ID, YaError::InvalidRemainingAccounts);
        let pos = ctx.accounts.position.key();
        let bump = [ctx.accounts.position.vault_authority_bump];
        let sd: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, pos.as_ref(), &bump]];
        // request_remove_insurance_fund_stake(market_index, amount): spot_market(w) if_stake(w) user_stats(w) authority(signer) if_vault(w)
        CpiCall::global(DRIFT_ID, "request_remove_insurance_fund_stake").arg(&MARKET_INDEX).arg(&shares)
            .account(ra[0].key(), false, true).account(ra[1].key(), false, true).account(ra[2].key(), false, true)
            .account(ctx.accounts.vault_authority.key(), true, false).account(ra[3].key(), false, true)
            .invoke_signed(&[ra[0].clone(), ra[1].clone(), ra[2].clone(),
                ctx.accounts.vault_authority.to_account_info(), ra[3].clone(), ra[4].clone()], sd)?;
        let now = Clock::get()?.unix_timestamp;
        let unlock = now.checked_add(unstaking_period).ok_or(YaError::MathOverflow)?;
        let p = ctx.accounts.position.key();
        let t = &mut ctx.accounts.ticket;
        require!(t.status != WithdrawalStatus::Pending, YaError::TicketAlreadyExists);
        t.position = p; t.shares = shares; t.min_amount_out = min_amount_out;
        t.unlock_ts = unlock; t.status = WithdrawalStatus::Pending; t.created_ts = now; t.bump = ctx.bumps.ticket;
        emit!(WithdrawRequested { position: p, shares, unlock_ts: unlock });
        Ok(())
    }

    /// Phase 2: settle after cooldown. remaining: 0 state 1 spot_market(w) 2 if_stake(w) 3 user_stats(w)
    ///          4 if_vault(w) 5 drift_signer 6 drift_program
    pub fn settle_withdrawal<'info>(ctx: Context<'info, WithdrawOp<'info>>, min_amount_out: u64) -> Result<()> {
        assert_active(&ctx.accounts.registry_entry.to_account_info(), &ctx.accounts.base_mint.key())?;
        let ra = ctx.remaining_accounts;
        require!(ra.len() >= 7, YaError::InvalidRemainingAccounts);
        let now = Clock::get()?.unix_timestamp;
        let (shares, unlock) = { let t = &ctx.accounts.ticket;
            require!(t.status == WithdrawalStatus::Pending, YaError::NothingToSettle); (t.shares, t.unlock_ts) };
        require!(now >= unlock, YaError::WithdrawalLocked);
        require_keys_eq!(ra[6].key(), DRIFT_ID, YaError::InvalidRemainingAccounts);
        let before = token_amount(&ctx.accounts.vault_token_account.to_account_info())?;
        let pos = ctx.accounts.position.key();
        let bump = [ctx.accounts.position.vault_authority_bump];
        let sd: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, pos.as_ref(), &bump]];
        // remove_insurance_fund_stake(market_index): state spot_market(w) if_stake(w) user_stats(w)
        //   authority(signer) if_vault(w) drift_signer user_token_account(w) token_program
        CpiCall::global(DRIFT_ID, "remove_insurance_fund_stake").arg(&MARKET_INDEX)
            .account(ra[0].key(), false, false).account(ra[1].key(), false, true).account(ra[2].key(), false, true)
            .account(ra[3].key(), false, true).account(ctx.accounts.vault_authority.key(), true, false)
            .account(ra[4].key(), false, true).account(ra[5].key(), false, false)
            .account(ctx.accounts.vault_token_account.key(), false, true).account(ctx.accounts.token_program.key(), false, false)
            .invoke_signed(&[ra[0].clone(), ra[1].clone(), ra[2].clone(), ra[3].clone(),
                ctx.accounts.vault_authority.to_account_info(), ra[4].clone(), ra[5].clone(),
                ctx.accounts.vault_token_account.to_account_info(), ctx.accounts.token_program.to_account_info(), ra[6].clone()], sd)?;
        let redeemed = token_amount(&ctx.accounts.vault_token_account.to_account_info())?
            .checked_sub(before).ok_or(YaError::MathOverflow)?;
        let decimals = mint_decimals(&ctx.accounts.base_mint.to_account_info())?;
        spl_transfer_checked(&ctx.accounts.token_program.to_account_info(),
            &ctx.accounts.vault_token_account.to_account_info(), &ctx.accounts.base_mint.to_account_info(),
            &ctx.accounts.owner_token_account.to_account_info(), &ctx.accounts.vault_authority.to_account_info(),
            redeemed, decimals, Some(sd))?;
        require!(redeemed >= min_amount_out, YaError::SlippageExceeded);
        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_sub(shares).ok_or(YaError::MathOverflow)?;
        p.value_updated_ts = now;
        let t = &mut ctx.accounts.ticket;
        t.status = WithdrawalStatus::Settled;
        emit!(WithdrawSettled { position: p.key(), amount_out: redeemed });
        Ok(())
    }

    /// View: if_shares * IF_vault_balance / total_if_shares (oracle-free). When a withdrawal is
    /// pending, capped at last_withdraw_request_value (§11 retroactive-price rule).
    /// remaining: 0 if_stake 1 spot_market 2 if_vault
    pub fn current_value<'info>(ctx: Context<'info, Op<'info>>) -> Result<()> {
        assert_active(&ctx.accounts.registry_entry.to_account_info(), &ctx.accounts.base_mint.key())?;
        let ra = ctx.remaining_accounts;
        require!(ra.len() >= 3, YaError::InvalidRemainingAccounts);
        let if_shares = read_u128(&ra[0], IF_STAKE_DISC, IFS_IF_SHARES)?;
        let pending_shares = read_u128(&ra[0], IF_STAKE_DISC, IFS_LAST_REQ_SHARES)?;
        let last_req_value = read_u64(&ra[0], IF_STAKE_DISC, IFS_LAST_REQ_VALUE)?;
        let total_shares = read_u128(&ra[1], SPOT_MARKET_DISC, SM_TOTAL_SHARES)?;
        let vault_balance = token_amount(&ra[2])?;
        let mut value = if total_shares == 0 {
            0
        } else {
            let v = mul_div_floor(vault_balance as u128, if_shares, total_shares).ok_or(YaError::MathOverflow)?;
            require!(v <= u64::MAX as u128, YaError::MathOverflow);
            v as u64
        };
        if pending_shares > 0 {
            value = value.min(last_req_value); // §11 cap
        }
        let p = &mut ctx.accounts.position;
        p.cached_value = value;
        p.value_updated_ts = Clock::get()?.unix_timestamp;
        report_value(value);
        emit!(ValueReported { position: p.key(), value });
        Ok(())
    }
}

// ── value math ──────────────────────────────────────────────
/// floor(a * b / d) for u128 operands via a 256-bit product + restoring 256/128 long division.
fn mul_div_floor(a: u128, b: u128, d: u128) -> Option<u128> {
    if d == 0 { return None; }
    let (hi, lo) = mul_u128(a, b);
    if hi >= d { return None; } // quotient would exceed u128
    let mut rem = hi;
    let mut quo: u128 = 0;
    let mut i: u32 = 128;
    while i > 0 {
        i -= 1;
        let carry = rem >> 127;
        rem = (rem << 1) | ((lo >> i) & 1);
        if carry == 1 || rem >= d {
            rem = rem.wrapping_sub(d);
            quo |= 1u128 << i;
        }
    }
    Some(quo)
}

fn mul_u128(a: u128, b: u128) -> (u128, u128) {
    let mask = u64::MAX as u128;
    let (a_lo, a_hi) = (a & mask, a >> 64);
    let (b_lo, b_hi) = (b & mask, b >> 64);
    let ll = a_lo * b_lo;
    let lh = a_lo * b_hi;
    let hl = a_hi * b_lo;
    let hh = a_hi * b_hi;
    let mid = (ll >> 64) + (lh & mask) + (hl & mask);
    let lo = (ll & mask) | ((mid & mask) << 64);
    let hi = hh + (lh >> 64) + (hl >> 64) + (mid >> 64);
    (hi, lo)
}

// ── helpers ─────────────────────────────────────────────────
fn assert_active(registry_entry: &AccountInfo, base_mint: &Pubkey) -> Result<()> {
    let entry = ya_registry::load_adapter_entry(registry_entry, &crate::ID)?;
    require!(entry.status == AdapterStatus::Active, YaError::AdapterNotActive);
    require_keys_eq!(entry.base_mint, *base_mint, YaError::BaseMintMismatch);
    Ok(())
}

fn validate_drift(ai: &AccountInfo, disc: [u8; 8]) -> Result<()> {
    require_keys_eq!(*ai.owner, DRIFT_ID, YaError::InvalidRemainingAccounts);
    let data = ai.try_borrow_data()?;
    require!(data.len() >= 8 && data[0..8] == disc, YaError::InvalidRemainingAccounts);
    Ok(())
}

fn read_u128(ai: &AccountInfo, disc: [u8; 8], off: usize) -> Result<u128> {
    validate_drift(ai, disc)?;
    let data = ai.try_borrow_data()?;
    require!(data.len() >= off + 16, YaError::InvalidRemainingAccounts);
    Ok(u128::from_le_bytes(data[off..off + 16].try_into().unwrap()))
}
fn read_u64(ai: &AccountInfo, disc: [u8; 8], off: usize) -> Result<u64> {
    validate_drift(ai, disc)?;
    let data = ai.try_borrow_data()?;
    require!(data.len() >= off + 8, YaError::InvalidRemainingAccounts);
    Ok(u64::from_le_bytes(data[off..off + 8].try_into().unwrap()))
}
fn read_i64(ai: &AccountInfo, disc: [u8; 8], off: usize) -> Result<i64> {
    Ok(read_u64(ai, disc, off)? as i64)
}

fn token_amount(ai: &AccountInfo) -> Result<u64> {
    let data = ai.try_borrow_data()?;
    require!(data.len() >= 72, YaError::InvalidRemainingAccounts);
    Ok(u64::from_le_bytes(data[64..72].try_into().unwrap()))
}
fn mint_decimals(ai: &AccountInfo) -> Result<u8> {
    let data = ai.try_borrow_data()?;
    require!(data.len() >= 45, YaError::InvalidRemainingAccounts);
    Ok(data[44])
}

#[allow(clippy::too_many_arguments)]
fn spl_transfer_checked<'info>(
    token_program: &AccountInfo<'info>, source: &AccountInfo<'info>, mint: &AccountInfo<'info>,
    dest: &AccountInfo<'info>, authority: &AccountInfo<'info>, amount: u64, decimals: u8,
    signer: Option<&[&[&[u8]]]>,
) -> Result<()> {
    let mut data = Vec::with_capacity(10);
    data.push(12u8);
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(decimals);
    let ix = Instruction {
        program_id: *token_program.key,
        accounts: vec![
            AccountMeta::new(*source.key, false), AccountMeta::new_readonly(*mint.key, false),
            AccountMeta::new(*dest.key, false), AccountMeta::new_readonly(*authority.key, true),
        ],
        data,
    };
    let infos = [source.clone(), mint.clone(), dest.clone(), authority.clone(), token_program.clone()];
    match signer { Some(s) => invoke_signed(&ix, &infos, s), None => invoke(&ix, &infos) }.map_err(Into::into)
}

// ── accounts ────────────────────────────────────────────────
#[derive(Accounts)]
pub struct InitializePosition<'info> {
    #[account(init_if_needed, payer = owner, space = Position::SPACE,
        seeds = [seeds::POSITION, owner.key().as_ref(), base_mint.key().as_ref()], bump)]
    pub position: Account<'info, Position>,
    /// CHECK: vault_authority PDA (Drift stake authority + protocol CPI signer).
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump)]
    pub vault_authority: UncheckedAccount<'info>,
    pub base_mint: InterfaceAccount<'info, Mint>,
    #[account(init_if_needed, payer = owner, seeds = [VAULT_USDC_SEED, position.key().as_ref()], bump,
        token::mint = base_mint, token::authority = vault_authority, token::token_program = token_program)]
    pub vault_usdc: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub token_program: Interface<'info, TokenInterface>,
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
    /// CHECK: base mint (USDC).
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: vault USDC token account (PDA).
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    pub owner: Signer<'info>,
    /// CHECK: owner USDC token account.
    #[account(mut)]
    pub owner_token_account: UncheckedAccount<'info>,
    /// CHECK: validated in assert_active.
    pub registry_entry: UncheckedAccount<'info>,
    /// CHECK: SPL token program.
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
    /// CHECK: vault USDC token account (PDA).
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: owner USDC token account.
    #[account(mut)]
    pub owner_token_account: UncheckedAccount<'info>,
    /// CHECK: validated in assert_active.
    pub registry_entry: UncheckedAccount<'info>,
    /// CHECK: SPL token program.
    pub token_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    #[account(init_if_needed, payer = owner, space = WithdrawalTicket::SPACE,
        seeds = [seeds::TICKET, position.key().as_ref()], bump)]
    pub ticket: Account<'info, WithdrawalTicket>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn if_value_math() {
        // vault 1_000_000, if_shares 250, total 1000 -> 250_000.
        assert_eq!(mul_div_floor(1_000_000, 250, 1000), Some(250_000));
        // large u128 shares stay exact: vault 30e6, if_shares 6e23, total 24e23 -> 7.5e6
        assert_eq!(mul_div_floor(30_000_000, 600_000_000_000_000_000_000_000, 2_400_000_000_000_000_000_000_000), Some(7_500_000));
        assert_eq!(mul_div_floor(5, 7, 0), None);
    }
}
