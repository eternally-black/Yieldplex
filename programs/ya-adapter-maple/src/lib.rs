// ============================================================
// Program:  ya-adapter-maple  (Yield Adapter Standard — Maple syrupUSDC adapter)
// Framework: Anchor 1.0.2
// Risk Level: 🔴 Critical — custodies syrupUSDC in the vault_authority PDA, CPIs USDC<->syrupUSDC
//             via one Orca Whirlpool swap, values syrupUSDC by an independent Chainlink feed.
// Security:  See programs/ya-adapter-maple/security-checklist.md
//
// Reference adapter #4 (swap-and-hold). syrupUSDC has no native synchronous Solana deposit (its
// lending lives on Ethereum; it's a Chainlink CCIP token), so the correct Solana primitive is a
// single direct swap on the deepest USDC<->syrupUSDC Orca Whirlpool:
//   deposit  -> transfer owner USDC -> vault, then `swap` USDC->syrupUSDC (a_to_b=false). Instant.
//   withdraw -> `swap` syrupUSDC->USDC (a_to_b=true) -> vault, then transfer -> owner.
//   current_value -> syrup_balance * Chainlink "SYRUPUSDC-USDC Exchange Rate" (NOT the Orca spot
//                    quote, NOT 1:1); fail-closed on a stale/invalid feed.
// Pool A = syrupUSDC, B = USDC. One CPI per op, invoke_signed by vault_authority.
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
    WithdrawSettled, YaError,
};
use ya_registry::AdapterStatus;

declare_id!("Ck9mwpX9kAjycbtN7jhD3s9xdHzUS2dwuV43g3BuBnD");

ya_interface::declare_ya_accounts!(); // Position + WithdrawalTicket (owned by this program)

/// Orca Whirlpool program + the deepest USDC<->syrupUSDC pool (A=syrupUSDC, B=USDC).
const ORCA_ID: Pubkey = pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
const POOL: Pubkey = pubkey!("6fteKNvMdv7tYmBoJHhj1jx6rHcEwC6RdSEmVpyS613J");
const SYRUP_MINT: Pubkey = pubkey!("AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj");
/// Chainlink Solana "SYRUPUSDC-USDC Exchange Rate" feed (Transmissions account) + its OCR2 store owner.
const CHAINLINK_FEED: Pubkey = pubkey!("CpNyiFt84q66665Kx64bobxZuMgZ2EecrhAJs1HikS2T");
const CHAINLINK_OWNER: Pubkey = pubkey!("HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny");
/// Max age of the Chainlink answer before we fail closed (seconds).
const MAX_STALE: i64 = 3600;
/// Orca swap sqrt-price bounds (no-limit per direction).
const MIN_SQRT_PRICE: u128 = 4295048016;
const MAX_SQRT_PRICE: u128 = 79226673515401279992447579055;

const VAULT_USDC_SEED: &[u8] = b"vault_usdc";
const VAULT_SYRUP_SEED: &[u8] = b"vault_syrup";

// remaining_accounts for deposit / withdraw (after prefix; withdraw prepends the ticket):
//   0 vault_syrup(w) · 1 whirlpool(w) · 2 token_vault_a(w) · 3 token_vault_b(w)
//   4 tick_array_0(w) · 5 tick_array_1(w) · 6 tick_array_2(w) · 7 oracle · 8 orca_program · 9 chainlink_feed
const M_VAULT_SYRUP: usize = 0;
const M_WHIRLPOOL: usize = 1;
const M_VAULT_A: usize = 2;
const M_VAULT_B: usize = 3;
const M_TICK0: usize = 4;
const M_TICK1: usize = 5;
const M_TICK2: usize = 6;
const M_ORACLE: usize = 7;
const M_PROGRAM: usize = 8;
const M_CHAINLINK: usize = 9;
const M_LEN: usize = 10;

#[program]
pub mod ya_adapter_maple {
    use super::*;

    /// Idempotent. Creates the Position, vault USDC account, and vault syrupUSDC account.
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

    /// Deposit: swap USDC -> syrupUSDC (a_to_b=false), hold syrupUSDC in the vault.
    pub fn deposit<'info>(
        ctx: Context<'info, Op<'info>>,
        amount: u64,
        min_position_out: u64,
    ) -> Result<()> {
        assert_active(
            &ctx.accounts.registry_entry.to_account_info(),
            &ctx.accounts.base_mint.key(),
        )?;
        let ra = ctx.remaining_accounts;
        require!(ra.len() >= M_LEN, YaError::InvalidRemainingAccounts);
        validate_orca(ra)?;
        validate_vault(
            &ctx.accounts.vault_token_account.key(),
            &ra[M_VAULT_SYRUP],
            &ctx.accounts.position.key(),
        )?;

        let decimals = mint_decimals(&ctx.accounts.base_mint.to_account_info())?;
        spl_transfer_checked(
            &ctx.accounts.token_program.to_account_info(),
            &ctx.accounts.owner_token_account.to_account_info(),
            &ctx.accounts.base_mint.to_account_info(),
            &ctx.accounts.vault_token_account.to_account_info(),
            &ctx.accounts.owner.to_account_info(),
            amount,
            decimals,
            None,
        )?;

        let before = token_amount(&ra[M_VAULT_SYRUP])?;
        // swap USDC (B) -> syrupUSDC (A): a_to_b=false, exact-input `amount` USDC, min syrup out.
        orca_swap(&ctx_swap(&ctx, ra), amount, min_position_out, MAX_SQRT_PRICE, true, false,
            &ctx.accounts.position, ctx.accounts.position.vault_authority_bump)?;
        let received = token_amount(&ra[M_VAULT_SYRUP])?
            .checked_sub(before)
            .ok_or(YaError::MathOverflow)?;
        require!(received >= min_position_out, YaError::SlippageExceeded);

        let now = Clock::get()?.unix_timestamp;
        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_add(received).ok_or(YaError::MathOverflow)?;
        p.cached_value = chainlink_value(p.shares, &ra[M_CHAINLINK], now)?;
        p.value_updated_ts = now;
        emit!(Deposited {
            position: p.key(),
            amount,
            value_after: p.cached_value,
        });
        Ok(())
    }

    /// Instant withdraw: swap syrupUSDC -> USDC (a_to_b=true), pay the owner, settle in one call.
    pub fn withdraw<'info>(
        ctx: Context<'info, WithdrawOp<'info>>,
        shares: u64,
        min_amount_out: u64,
    ) -> Result<()> {
        assert_active(
            &ctx.accounts.registry_entry.to_account_info(),
            &ctx.accounts.base_mint.key(),
        )?;
        let ra = ctx.remaining_accounts;
        require!(ra.len() >= M_LEN, YaError::InvalidRemainingAccounts);
        require!(shares <= ctx.accounts.position.shares, YaError::SlippageExceeded);
        validate_orca(ra)?;
        validate_vault(
            &ctx.accounts.vault_token_account.key(),
            &ra[M_VAULT_SYRUP],
            &ctx.accounts.position.key(),
        )?;

        let usdc_before = token_amount(&ctx.accounts.vault_token_account.to_account_info())?;
        // swap syrupUSDC (A) -> USDC (B): a_to_b=true, exact-input `shares` syrup, min USDC out.
        orca_swap_w(&ctx, ra, shares, min_amount_out, MIN_SQRT_PRICE, true, true)?;
        let redeemed = token_amount(&ctx.accounts.vault_token_account.to_account_info())?
            .checked_sub(usdc_before)
            .ok_or(YaError::MathOverflow)?;

        let decimals = mint_decimals(&ctx.accounts.base_mint.to_account_info())?;
        let position_key = ctx.accounts.position.key();
        let va_bump = [ctx.accounts.position.vault_authority_bump];
        let signer: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, position_key.as_ref(), &va_bump]];
        spl_transfer_checked(
            &ctx.accounts.token_program.to_account_info(),
            &ctx.accounts.vault_token_account.to_account_info(),
            &ctx.accounts.base_mint.to_account_info(),
            &ctx.accounts.owner_token_account.to_account_info(),
            &ctx.accounts.vault_authority.to_account_info(),
            redeemed,
            decimals,
            Some(signer),
        )?;
        require!(redeemed >= min_amount_out, YaError::SlippageExceeded);

        let now = Clock::get()?.unix_timestamp;
        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_sub(shares).ok_or(YaError::MathOverflow)?;
        p.cached_value = chainlink_value(p.shares, &ra[M_CHAINLINK], now)?;
        p.value_updated_ts = now;

        let t = &mut ctx.accounts.ticket;
        t.position = p.key();
        t.shares = shares;
        t.min_amount_out = min_amount_out;
        t.unlock_ts = 0;
        t.status = WithdrawalStatus::Settled;
        t.created_ts = now;
        t.bump = ctx.bumps.ticket;

        emit!(WithdrawSettled {
            position: p.key(),
            amount_out: redeemed,
        });
        Ok(())
    }

    /// Instant adapter: nothing to settle.
    pub fn settle_withdrawal(_ctx: Context<Op>, _min_amount_out: u64) -> Result<()> {
        Err(YaError::NothingToSettle.into())
    }

    /// View: syrupUSDC balance * Chainlink exchange rate (fail-closed on stale/invalid).
    pub fn current_value<'info>(ctx: Context<'info, Op<'info>>) -> Result<()> {
        assert_active(
            &ctx.accounts.registry_entry.to_account_info(),
            &ctx.accounts.base_mint.key(),
        )?;
        let ra = ctx.remaining_accounts;
        require!(!ra.is_empty(), YaError::InvalidRemainingAccounts);
        let now = Clock::get()?.unix_timestamp;
        let value = chainlink_value(ctx.accounts.position.shares, &ra[0], now)?;
        let p = &mut ctx.accounts.position;
        p.cached_value = value;
        p.value_updated_ts = now;
        report_value(value);
        emit!(ValueReported {
            position: p.key(),
            value,
        });
        Ok(())
    }
}

// ── Chainlink value (NOT the Orca quote) ─────────────────────
/// syrupUSDC -> USDC via the Chainlink "SYRUPUSDC-USDC Exchange Rate" feed. Validates the feed
/// account + its OCR2 store owner, fails closed on a stale/non-positive answer (§11). The feed's
/// Transmissions account holds a single live round: decimals@138, timestamp(u32)@208, answer(i128)@216.
fn chainlink_value(shares: u64, feed: &AccountInfo, now: i64) -> Result<u64> {
    require_keys_eq!(*feed.key, CHAINLINK_FEED, YaError::InvalidRemainingAccounts);
    require_keys_eq!(*feed.owner, CHAINLINK_OWNER, YaError::InvalidRemainingAccounts);
    let data = feed.try_borrow_data()?;
    require!(data.len() >= 232, YaError::OracleStale);
    let decimals = data[138];
    require!(decimals <= 18, YaError::OracleStale);
    let ts = u32::from_le_bytes(data[208..212].try_into().unwrap()) as i64;
    require!(ts > 0 && now >= ts && now - ts <= MAX_STALE, YaError::OracleStale);
    let answer = i128::from_le_bytes(data[216..232].try_into().unwrap());
    require!(answer > 0, YaError::OracleStale);
    let scale = 10u64.checked_pow(decimals as u32).ok_or(YaError::MathOverflow)?;
    let v = mul_div_u64(shares, answer as u128, scale).ok_or(YaError::MathOverflow)?;
    require!(v <= u64::MAX as u128, YaError::MathOverflow);
    Ok(v as u64)
}

/// floor(a * b / divisor): a:u64, b:u128, divisor:u64. 192-bit product / single u64 limb (exact).
fn mul_div_u64(a: u64, b: u128, divisor: u64) -> Option<u128> {
    if divisor == 0 {
        return None;
    }
    let mask = u64::MAX as u128;
    let lo = (a as u128) * (b & mask);
    let hi = (a as u128) * (b >> 64);
    let p0 = (lo & mask) as u64;
    let mid = (lo >> 64) + (hi & mask);
    let p1 = (mid & mask) as u64;
    let p2 = ((hi >> 64) + (mid >> 64)) as u64;
    let d = divisor as u128;
    let mut rem: u128 = 0;
    let mut q = [0u64; 3];
    for (i, &limb) in [p2, p1, p0].iter().enumerate() {
        let acc = (rem << 64) | (limb as u128);
        q[i] = (acc / d) as u64;
        rem = acc % d;
    }
    if q[0] != 0 {
        return None;
    }
    Some(((q[1] as u128) << 64) | (q[2] as u128))
}

// ── Orca swap CPI (v1 `swap`, 12 accounts) ───────────────────
struct SwapCtx<'a, 'info> {
    token_program: AccountInfo<'info>,
    token_authority: AccountInfo<'info>,
    owner_a: AccountInfo<'info>, // syrupUSDC side (token A)
    owner_b: AccountInfo<'info>, // USDC side (token B)
    ra: &'a [AccountInfo<'info>],
}
fn ctx_swap<'a, 'info>(ctx: &'a Context<'info, Op<'info>>, ra: &'a [AccountInfo<'info>]) -> SwapCtx<'a, 'info> {
    SwapCtx {
        token_program: ctx.accounts.token_program.to_account_info(),
        token_authority: ctx.accounts.vault_authority.to_account_info(),
        owner_a: ra[M_VAULT_SYRUP].clone(),
        owner_b: ctx.accounts.vault_token_account.to_account_info(),
        ra,
    }
}

#[allow(clippy::too_many_arguments)]
fn orca_swap(
    s: &SwapCtx,
    amount: u64,
    other_threshold: u64,
    sqrt_limit: u128,
    amount_in: bool,
    a_to_b: bool,
    position: &Account<Position>,
    va_bump: u8,
) -> Result<()> {
    let position_key = position.key();
    let bump = [va_bump];
    let signer: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, position_key.as_ref(), &bump]];
    let ra = s.ra;
    CpiCall::global(ORCA_ID, "swap")
        .arg(&amount)
        .arg(&other_threshold)
        .arg(&sqrt_limit)
        .arg(&amount_in)
        .arg(&a_to_b)
        .account(s.token_program.key(), false, false) // token_program
        .account(s.token_authority.key(), true, false) // token_authority
        .account(ra[M_WHIRLPOOL].key(), false, true)
        .account(s.owner_a.key(), false, true) // token_owner_account_a (syrup)
        .account(ra[M_VAULT_A].key(), false, true) // token_vault_a
        .account(s.owner_b.key(), false, true) // token_owner_account_b (USDC)
        .account(ra[M_VAULT_B].key(), false, true) // token_vault_b
        .account(ra[M_TICK0].key(), false, true)
        .account(ra[M_TICK1].key(), false, true)
        .account(ra[M_TICK2].key(), false, true)
        .account(ra[M_ORACLE].key(), false, false) // oracle (not writable in v1)
        .account(ra[M_PROGRAM].key(), false, false) // whirlpool_program
        .invoke_signed(
            &[
                s.token_program.clone(),
                s.token_authority.clone(),
                ra[M_WHIRLPOOL].clone(),
                s.owner_a.clone(),
                ra[M_VAULT_A].clone(),
                s.owner_b.clone(),
                ra[M_VAULT_B].clone(),
                ra[M_TICK0].clone(),
                ra[M_TICK1].clone(),
                ra[M_TICK2].clone(),
                ra[M_ORACLE].clone(),
                ra[M_PROGRAM].clone(),
            ],
            signer,
        )
}

fn orca_swap_w<'info>(
    ctx: &Context<'info, WithdrawOp<'info>>,
    ra: &[AccountInfo<'info>],
    amount: u64,
    other_threshold: u64,
    sqrt_limit: u128,
    amount_in: bool,
    a_to_b: bool,
) -> Result<()> {
    let s = SwapCtx {
        token_program: ctx.accounts.token_program.to_account_info(),
        token_authority: ctx.accounts.vault_authority.to_account_info(),
        owner_a: ra[M_VAULT_SYRUP].clone(),
        owner_b: ctx.accounts.vault_token_account.to_account_info(),
        ra,
    };
    orca_swap(&s, amount, other_threshold, sqrt_limit, amount_in, a_to_b,
        &ctx.accounts.position, ctx.accounts.position.vault_authority_bump)
}

// ── validation / token helpers ──────────────────────────────
fn assert_active(registry_entry: &AccountInfo, base_mint: &Pubkey) -> Result<()> {
    let entry = ya_registry::load_adapter_entry(registry_entry, &crate::ID)?;
    require!(entry.status == AdapterStatus::Active, YaError::AdapterNotActive);
    require_keys_eq!(entry.base_mint, *base_mint, YaError::BaseMintMismatch);
    Ok(())
}

fn validate_orca(ra: &[AccountInfo]) -> Result<()> {
    require_keys_eq!(ra[M_WHIRLPOOL].key(), POOL, YaError::InvalidRemainingAccounts);
    require_keys_eq!(ra[M_PROGRAM].key(), ORCA_ID, YaError::InvalidRemainingAccounts);
    Ok(())
}

fn validate_vault(vault_usdc: &Pubkey, vault_syrup: &AccountInfo, position: &Pubkey) -> Result<()> {
    let usdc_pda = Pubkey::find_program_address(&[VAULT_USDC_SEED, position.as_ref()], &crate::ID).0;
    require_keys_eq!(*vault_usdc, usdc_pda, YaError::InvalidRemainingAccounts);
    let syrup_pda = Pubkey::find_program_address(&[VAULT_SYRUP_SEED, position.as_ref()], &crate::ID).0;
    require_keys_eq!(*vault_syrup.key, syrup_pda, YaError::InvalidRemainingAccounts);
    let data = vault_syrup.try_borrow_data()?;
    require!(data.len() >= 32, YaError::InvalidRemainingAccounts);
    let mint = Pubkey::new_from_array(data[0..32].try_into().unwrap());
    require_keys_eq!(mint, SYRUP_MINT, YaError::InvalidRemainingAccounts);
    Ok(())
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
    token_program: &AccountInfo<'info>,
    source: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    dest: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
    decimals: u8,
    signer: Option<&[&[&[u8]]]>,
) -> Result<()> {
    let mut data = Vec::with_capacity(10);
    data.push(12u8);
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(decimals);
    let ix = Instruction {
        program_id: *token_program.key,
        accounts: vec![
            AccountMeta::new(*source.key, false),
            AccountMeta::new_readonly(*mint.key, false),
            AccountMeta::new(*dest.key, false),
            AccountMeta::new_readonly(*authority.key, true),
        ],
        data,
    };
    let infos = [
        source.clone(),
        mint.clone(),
        dest.clone(),
        authority.clone(),
        token_program.clone(),
    ];
    match signer {
        Some(seeds) => invoke_signed(&ix, &infos, seeds),
        None => invoke(&ix, &infos),
    }
    .map_err(Into::into)
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
    /// CHECK: vault_authority PDA (token authority + protocol CPI signer).
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump)]
    pub vault_authority: UncheckedAccount<'info>,
    pub base_mint: InterfaceAccount<'info, Mint>,
    /// syrupUSDC mint.
    pub syrup_mint: InterfaceAccount<'info, Mint>,
    #[account(
        init_if_needed,
        payer = owner,
        seeds = [VAULT_USDC_SEED, position.key().as_ref()],
        bump,
        token::mint = base_mint,
        token::authority = vault_authority,
        token::token_program = token_program,
    )]
    pub vault_usdc: InterfaceAccount<'info, TokenAccount>,
    #[account(
        init_if_needed,
        payer = owner,
        seeds = [VAULT_SYRUP_SEED, position.key().as_ref()],
        bump,
        token::mint = syrup_mint,
        token::authority = vault_authority,
        token::token_program = token_program,
    )]
    pub vault_syrup: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

/// Standard prefix (§4.3) — deposit / settle_withdrawal / current_value.
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
    /// CHECK: base mint (USDC).
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: vault USDC token account (PDA); validated in validate_vault.
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    pub owner: Signer<'info>,
    /// CHECK: owner USDC token account.
    #[account(mut)]
    pub owner_token_account: UncheckedAccount<'info>,
    /// CHECK: validated in assert_active via ya_registry::load_adapter_entry.
    pub registry_entry: UncheckedAccount<'info>,
    /// CHECK: SPL token program (forwarded).
    pub token_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// Withdraw adds the standard withdrawal ticket (first remaining account from the dispatcher).
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
    /// CHECK: base mint (USDC).
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: vault USDC token account (PDA); validated in validate_vault.
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: owner USDC token account (receives the redeemed USDC).
    #[account(mut)]
    pub owner_token_account: UncheckedAccount<'info>,
    /// CHECK: validated in assert_active via ya_registry::load_adapter_entry.
    pub registry_entry: UncheckedAccount<'info>,
    /// CHECK: SPL token program (forwarded).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syrup_value_matches_chainlink_rate() {
        // answer 1167864 (decimals 6 => 1.167864 USDC/syrup). 21_407_000 syrup -> ~25 USDC.
        assert_eq!(mul_div_u64(21_407_000, 1_167_864, 1_000_000), Some(25_000_000));
        assert_eq!(mul_div_u64(0, 1_167_864, 1_000_000), Some(0));
        assert_eq!(mul_div_u64(1_000_000, 1_167_864, 1_000_000), Some(1_167_864));
    }
}
