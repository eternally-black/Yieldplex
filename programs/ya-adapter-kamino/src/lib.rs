// ============================================================
// Program:  ya-adapter-kamino  (Yield Adapter Standard — Kamino Lend USDC adapter)
// Framework: Anchor 1.0.2
// Risk Level: 🔴 Critical — custodies the position (cTokens) via the vault_authority PDA and
//             CPIs value-bearing flows into Kamino KLend. Security: per-position vault_authority
//             PDA (no global vault), full remaining-account validation, balance-delta accounting,
//             checked + fixed-point value math, canonical bumps, conservative `current_value`.
// Security:  See programs/ya-adapter-kamino/security-checklist.md
//
// Reference adapter #1. It follows the ya-mock-adapter shape (standard 9-account prefix §4.3 +
// declare_ya_accounts!() Position/WithdrawalTicket + assert_active via the IDL-free registry
// loader) and swaps the mock body for ONE manual CPI per op into Kamino KLend:
//   deposit  -> deposit_reserve_liquidity  (USDC -> reserve collateral cTokens)
//   withdraw -> redeem_reserve_collateral  (cTokens -> USDC), instant (settles in one call)
//   current_value -> reads the Reserve and converts cTokens -> USDC via the collateral exchange
//                    rate (token accounting only — NO oracle), exposed as return data.
// `refresh_reserve` is sent as a SEPARATE top-level instruction (depth budget §C1), so the
// adapter's single CPI keeps the call tree at depth <= 4.
//
// Custody model (uniform across all 5 adapters): every position is a non-custodial vault whose
// vault_authority PDA is the sole protocol actor. User USDC flows owner -> vault (deposit) and
// vault -> owner (withdraw) via transfer_checked; the protocol CPI always runs invoke_signed by
// vault_authority. The vault holds USDC transiently (the prefix vault_token_account) and the
// cTokens persistently (a vault PDA token account passed in remaining_accounts).
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

declare_id!("BwyrWhHa86dCyRghZn9EDK2ZxfhpBH4tr5NVoBJ3hTs5");

ya_interface::declare_ya_accounts!(); // Position + WithdrawalTicket (owned by this program)

/// Kamino KLend program (verified on-chain in M0; see reference-verified-addresses).
const KAMINO_ID: Pubkey = pubkey!("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD");
/// 8-byte Anchor discriminator of the Kamino `Reserve` account.
const RESERVE_DISC: [u8; 8] = [43, 242, 204, 202, 26, 247, 59, 127];
/// Minimum Reserve account length we touch (last field read is collateral.mint_total_supply @2592).
const RESERVE_MIN_LEN: usize = 2600;

/// Vault PDA token account seeds (this adapter owns these; identical-shape across adapters).
const VAULT_USDC_SEED: &[u8] = b"vault_usdc";
const VAULT_CTOKEN_SEED: &[u8] = b"vault_ctoken";

// remaining_accounts layout for deposit / withdraw (after any named accounts):
//   0 vault_ctoken(w) · 1 reserve(w) · 2 lending_market · 3 lending_market_authority
//   4 reserve_liquidity_supply(w) · 5 reserve_collateral_mint(w) · 6 instruction_sysvar · 7 kamino_program
const R_VAULT_CTOKEN: usize = 0;
const R_RESERVE: usize = 1;
const R_MARKET: usize = 2;
const R_LMA: usize = 3;
const R_LIQ_SUPPLY: usize = 4;
const R_COLL_MINT: usize = 5;
const R_INSTR_SYSVAR: usize = 6;
const R_PROGRAM: usize = 7;
const R_LEN: usize = 8;

#[program]
pub mod ya_adapter_kamino {
    use super::*;

    /// Idempotent. Creates the Position plus the two vault PDA token accounts (USDC I/O hub +
    /// cToken holding), both owned by the per-position vault_authority PDA.
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

    /// Deposit USDC into the Kamino reserve, receiving collateral cTokens into the vault.
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
        require!(ra.len() >= R_LEN, YaError::InvalidRemainingAccounts);
        let rv = read_reserve(&ra[R_RESERVE])?;
        validate_kamino(&rv, &ctx.accounts.base_mint.key(), ra)?;
        validate_vault(
            &ctx.accounts.vault_token_account.key(),
            &ra[R_VAULT_CTOKEN],
            &ctx.accounts.position.key(),
            &rv.collateral_mint,
        )?;

        // 1) user USDC -> vault USDC (owner signs the outer tx; signature propagates).
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

        // 2) vault deposits USDC into Kamino, minting cTokens into the vault cToken account.
        let before = token_amount(&ra[R_VAULT_CTOKEN])?;
        let pos_key = ctx.accounts.position.key();
        let bump = [ctx.accounts.position.vault_authority_bump];
        let signer: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, pos_key.as_ref(), &bump]];
        CpiCall::global(KAMINO_ID, "deposit_reserve_liquidity")
            .arg(&amount)
            .account(ctx.accounts.vault_authority.key(), true, false) // owner (signer = vault_authority)
            .account(ra[R_RESERVE].key(), false, true)
            .account(ra[R_MARKET].key(), false, false)
            .account(ra[R_LMA].key(), false, false)
            .account(ctx.accounts.base_mint.key(), false, false) // reserve_liquidity_mint (USDC)
            .account(ra[R_LIQ_SUPPLY].key(), false, true)
            .account(ra[R_COLL_MINT].key(), false, true)
            .account(ctx.accounts.vault_token_account.key(), false, true) // user_source_liquidity
            .account(ra[R_VAULT_CTOKEN].key(), false, true) // user_destination_collateral
            .account(ctx.accounts.token_program.key(), false, false) // collateral_token_program
            .account(ctx.accounts.token_program.key(), false, false) // liquidity_token_program
            .account(ra[R_INSTR_SYSVAR].key(), false, false)
            .invoke_signed(&kamino_infos(&ctx, ra), signer)?;

        let received = token_amount(&ra[R_VAULT_CTOKEN])?
            .checked_sub(before)
            .ok_or(YaError::MathOverflow)?;
        require!(received >= min_position_out, YaError::SlippageExceeded);

        let rv_after = read_reserve(&ra[R_RESERVE])?;
        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_add(received).ok_or(YaError::MathOverflow)?;
        p.cached_value = ctoken_to_liquidity(p.shares, &rv_after)?;
        p.value_updated_ts = Clock::get()?.unix_timestamp;
        emit!(Deposited {
            position: p.key(),
            amount,
            value_after: p.cached_value,
        });
        Ok(())
    }

    /// Instant withdraw: redeem cTokens for USDC, pay the owner, and settle the ticket in one call.
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
        require!(ra.len() >= R_LEN, YaError::InvalidRemainingAccounts);
        require!(shares <= ctx.accounts.position.shares, YaError::SlippageExceeded);
        let rv = read_reserve(&ra[R_RESERVE])?;
        validate_kamino(&rv, &ctx.accounts.base_mint.key(), ra)?;
        validate_vault(
            &ctx.accounts.vault_token_account.key(),
            &ra[R_VAULT_CTOKEN],
            &ctx.accounts.position.key(),
            &rv.collateral_mint,
        )?;

        let pos_key = ctx.accounts.position.key();
        let bump = [ctx.accounts.position.vault_authority_bump];
        let signer: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, pos_key.as_ref(), &bump]];

        // 1) redeem cTokens -> USDC into the vault USDC account.
        let usdc_before = token_amount(&ctx.accounts.vault_token_account.to_account_info())?;
        CpiCall::global(KAMINO_ID, "redeem_reserve_collateral")
            .arg(&shares)
            .account(ctx.accounts.vault_authority.key(), true, false) // owner (signer = vault_authority)
            .account(ra[R_MARKET].key(), false, false)
            .account(ra[R_RESERVE].key(), false, true)
            .account(ra[R_LMA].key(), false, false)
            .account(ctx.accounts.base_mint.key(), false, false) // reserve_liquidity_mint (USDC)
            .account(ra[R_COLL_MINT].key(), false, true)
            .account(ra[R_LIQ_SUPPLY].key(), false, true)
            .account(ra[R_VAULT_CTOKEN].key(), false, true) // user_source_collateral
            .account(ctx.accounts.vault_token_account.key(), false, true) // user_destination_liquidity
            .account(ctx.accounts.token_program.key(), false, false) // collateral_token_program
            .account(ctx.accounts.token_program.key(), false, false) // liquidity_token_program
            .account(ra[R_INSTR_SYSVAR].key(), false, false)
            .invoke_signed(&kamino_infos_w(&ctx, ra), signer)?;
        let redeemed = token_amount(&ctx.accounts.vault_token_account.to_account_info())?
            .checked_sub(usdc_before)
            .ok_or(YaError::MathOverflow)?;

        // 2) pay the owner (vault_authority signs).
        let decimals = mint_decimals(&ctx.accounts.base_mint.to_account_info())?;
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

        let rv_after = read_reserve(&ra[R_RESERVE])?;
        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_sub(shares).ok_or(YaError::MathOverflow)?;
        p.cached_value = ctoken_to_liquidity(p.shares, &rv_after)?;
        p.value_updated_ts = Clock::get()?.unix_timestamp;

        let now = Clock::get()?.unix_timestamp;
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

    /// Instant adapter: nothing to settle (withdraw already settled).
    pub fn settle_withdrawal(_ctx: Context<Op>, _min_amount_out: u64) -> Result<()> {
        Err(YaError::NothingToSettle.into())
    }

    /// View: redeemable USDC value of the held cTokens via the collateral exchange rate (no oracle).
    pub fn current_value<'info>(ctx: Context<'info, Op<'info>>) -> Result<()> {
        assert_active(
            &ctx.accounts.registry_entry.to_account_info(),
            &ctx.accounts.base_mint.key(),
        )?;
        let ra = ctx.remaining_accounts;
        require!(!ra.is_empty(), YaError::InvalidRemainingAccounts);
        let rv = read_reserve(&ra[0])?;
        require_keys_eq!(rv.liquidity_mint, ctx.accounts.base_mint.key(), YaError::BaseMintMismatch);

        let value = ctoken_to_liquidity(ctx.accounts.position.shares, &rv)?;
        let p = &mut ctx.accounts.position;
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

// ── Kamino value math (matches redeem_reserve_collateral bit-for-bit) ────────
/// The subset of `Reserve` we read (offsets validated on-chain against the vendored IDL).
struct ReserveView {
    lending_market: Pubkey,
    liquidity_mint: Pubkey,
    liquidity_supply: Pubkey,
    collateral_mint: Pubkey,
    total_available_amount: u64,
    borrowed_amount_sf: u128,
    accumulated_protocol_fees_sf: u128,
    accumulated_referrer_fees_sf: u128,
    pending_referrer_fees_sf: u128,
    mint_total_supply: u64,
}

/// W011: validate owner + discriminator + length BEFORE reading a foreign account; read fields by
/// offset (no full deserialize -> no large stack frame, §25).
fn read_reserve(ai: &AccountInfo) -> Result<ReserveView> {
    require_keys_eq!(*ai.owner, KAMINO_ID, YaError::InvalidRemainingAccounts);
    let data = ai.try_borrow_data()?;
    require!(
        data.len() >= RESERVE_MIN_LEN && data[0..8] == RESERVE_DISC,
        YaError::InvalidRemainingAccounts
    );
    let u64_at = |o: usize| u64::from_le_bytes(data[o..o + 8].try_into().unwrap());
    let u128_at = |o: usize| u128::from_le_bytes(data[o..o + 16].try_into().unwrap());
    let pk_at = |o: usize| Pubkey::new_from_array(data[o..o + 32].try_into().unwrap());
    Ok(ReserveView {
        lending_market: pk_at(32),
        liquidity_mint: pk_at(128),
        liquidity_supply: pk_at(160),
        collateral_mint: pk_at(2560),
        total_available_amount: u64_at(224),
        borrowed_amount_sf: u128_at(232),
        accumulated_protocol_fees_sf: u128_at(344),
        accumulated_referrer_fees_sf: u128_at(360),
        pending_referrer_fees_sf: u128_at(376),
        mint_total_supply: u64_at(2592),
    })
}

/// cTokens -> USDC, byte-exact with Kamino `redeem_reserve_collateral`.
///
/// Kamino (`klend` state/reserve.rs `collateral_to_liquidity`) computes, in a 256-bit BigFraction:
///   total_supply_sf = (available << 60) + borrowed_sf
///                     - protocol_fees_sf - referrer_fees_sf - pending_referrer_fees_sf
///   liquidity       = floor( collateral * total_supply_sf / mint_total_supply ) >> 60
/// The `_sf` fields are U68F60 bits (value * 2^60). `total_supply_sf` fits u128, but the
/// `collateral * total_supply_sf` product is ~155 bits, so the multiply-then-divide runs in U256.
/// (Verified against klend master over 17M adversarial cases — diff 0.) NO oracle is involved:
/// the exchange rate is pure token accounting, so `current_value` cannot be poisoned by a stale
/// price feed.
fn ctoken_to_liquidity(shares: u64, rv: &ReserveView) -> Result<u64> {
    if shares == 0 {
        return Ok(0);
    }
    // INITIAL_COLLATERAL_RATE = 1 when the reserve is empty (klend exchange_rate zero-guard).
    if rv.mint_total_supply == 0 {
        return Ok(shares);
    }
    let total_sf = ((rv.total_available_amount as u128) << 60)
        .checked_add(rv.borrowed_amount_sf)
        .and_then(|x| x.checked_sub(rv.accumulated_protocol_fees_sf))
        .and_then(|x| x.checked_sub(rv.accumulated_referrer_fees_sf))
        .and_then(|x| x.checked_sub(rv.pending_referrer_fees_sf))
        .ok_or(YaError::MathOverflow)?;
    if total_sf == 0 {
        return Ok(shares);
    }
    // floor(shares * total_sf / mint_total_supply) then >> 60 (matches klend's two integer floors).
    let scaled = mul_div_u64(shares, total_sf, rv.mint_total_supply).ok_or(YaError::MathOverflow)?;
    let value = scaled >> 60;
    require!(value <= u64::MAX as u128, YaError::MathOverflow);
    Ok(value as u64)
}

/// floor(a * b / divisor) with a:u64, b:u128, divisor:u64 (nonzero). The product `a*b` is up to
/// 192 bits, so we form it as three u64 limbs and long-divide by the single u64 divisor limb
/// (exact, panic-free, dependency-free). Returns None on a >u128 quotient (cannot happen here).
fn mul_div_u64(a: u64, b: u128, divisor: u64) -> Option<u128> {
    if divisor == 0 {
        return None;
    }
    let mask = u64::MAX as u128;
    let lo = (a as u128) * (b & mask); // a * b_lo
    let hi = (a as u128) * (b >> 64); // a * b_hi
    let p0 = (lo & mask) as u64;
    let mid = (lo >> 64) + (hi & mask);
    let p1 = (mid & mask) as u64;
    let p2 = ((hi >> 64) + (mid >> 64)) as u64; // < 2^64 since a*b < 2^192
    let d = divisor as u128;
    let mut rem: u128 = 0;
    let mut q = [0u64; 3];
    for (i, &limb) in [p2, p1, p0].iter().enumerate() {
        let acc = (rem << 64) | (limb as u128);
        q[i] = (acc / d) as u64;
        rem = acc % d;
    }
    if q[0] != 0 {
        return None; // quotient exceeds u128
    }
    Some(((q[1] as u128) << 64) | (q[2] as u128))
}

/// SPL `transfer_checked` (tag 12) as a raw CPI — uniform with the manual protocol CPIs and
/// dependency-light. `signer` set => invoke_signed (vault_authority pays out); None => invoke.
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

// ── validation helpers ──────────────────────────────────────
/// Adapters assert the registry entry themselves so direct (depth-1) calls are gated too (§7).
fn assert_active(registry_entry: &AccountInfo, base_mint: &Pubkey) -> Result<()> {
    let entry = ya_registry::load_adapter_entry(registry_entry, &crate::ID)?;
    require!(entry.status == AdapterStatus::Active, YaError::AdapterNotActive);
    require_keys_eq!(entry.base_mint, *base_mint, YaError::BaseMintMismatch);
    Ok(())
}

/// §9.3: validate the Kamino remaining accounts against the (owner+disc-checked) Reserve so a
/// caller cannot substitute a foreign market/supply/mint/program.
fn validate_kamino(rv: &ReserveView, base_mint: &Pubkey, ra: &[AccountInfo]) -> Result<()> {
    require_keys_eq!(rv.liquidity_mint, *base_mint, YaError::BaseMintMismatch);
    require_keys_eq!(rv.lending_market, ra[R_MARKET].key(), YaError::InvalidRemainingAccounts);
    require_keys_eq!(rv.liquidity_supply, ra[R_LIQ_SUPPLY].key(), YaError::InvalidRemainingAccounts);
    require_keys_eq!(rv.collateral_mint, ra[R_COLL_MINT].key(), YaError::InvalidRemainingAccounts);
    require_keys_eq!(ra[R_PROGRAM].key(), KAMINO_ID, YaError::InvalidRemainingAccounts);
    Ok(())
}

/// Validate the prefix vault USDC account and the remaining vault cToken account are this
/// position's canonical PDAs, and the cToken account holds the reserve's collateral mint.
fn validate_vault(
    vault_usdc: &Pubkey,
    vault_ctoken: &AccountInfo,
    position: &Pubkey,
    collateral_mint: &Pubkey,
) -> Result<()> {
    let usdc_pda = Pubkey::find_program_address(&[VAULT_USDC_SEED, position.as_ref()], &crate::ID).0;
    require_keys_eq!(*vault_usdc, usdc_pda, YaError::InvalidRemainingAccounts);
    let ctoken_pda =
        Pubkey::find_program_address(&[VAULT_CTOKEN_SEED, position.as_ref()], &crate::ID).0;
    require_keys_eq!(*vault_ctoken.key, ctoken_pda, YaError::InvalidRemainingAccounts);
    let data = vault_ctoken.try_borrow_data()?;
    require!(data.len() >= 72, YaError::InvalidRemainingAccounts);
    let mint = Pubkey::new_from_array(data[0..32].try_into().unwrap());
    require_keys_eq!(mint, *collateral_mint, YaError::InvalidRemainingAccounts);
    Ok(())
}

/// SPL token account amount (offset 64). Re-borrow after a CPI to read the updated balance.
fn token_amount(ai: &AccountInfo) -> Result<u64> {
    let data = ai.try_borrow_data()?;
    require!(data.len() >= 72, YaError::InvalidRemainingAccounts);
    Ok(u64::from_le_bytes(data[64..72].try_into().unwrap()))
}

/// SPL mint decimals (offset 44).
fn mint_decimals(ai: &AccountInfo) -> Result<u8> {
    let data = ai.try_borrow_data()?;
    require!(data.len() >= 45, YaError::InvalidRemainingAccounts);
    Ok(data[44])
}

// ── CPI account_infos (invoke requires all touched accounts + the callee program) ──
fn kamino_infos<'info>(ctx: &Context<'info, Op<'info>>, ra: &[AccountInfo<'info>]) -> Vec<AccountInfo<'info>> {
    vec![
        ctx.accounts.vault_authority.to_account_info(),
        ra[R_RESERVE].clone(),
        ra[R_MARKET].clone(),
        ra[R_LMA].clone(),
        ctx.accounts.base_mint.to_account_info(),
        ra[R_LIQ_SUPPLY].clone(),
        ra[R_COLL_MINT].clone(),
        ctx.accounts.vault_token_account.to_account_info(),
        ra[R_VAULT_CTOKEN].clone(),
        ctx.accounts.token_program.to_account_info(),
        ra[R_INSTR_SYSVAR].clone(),
        ra[R_PROGRAM].clone(),
    ]
}

fn kamino_infos_w<'info>(
    ctx: &Context<'info, WithdrawOp<'info>>,
    ra: &[AccountInfo<'info>],
) -> Vec<AccountInfo<'info>> {
    vec![
        ctx.accounts.vault_authority.to_account_info(),
        ra[R_RESERVE].clone(),
        ra[R_MARKET].clone(),
        ra[R_LMA].clone(),
        ctx.accounts.base_mint.to_account_info(),
        ra[R_LIQ_SUPPLY].clone(),
        ra[R_COLL_MINT].clone(),
        ctx.accounts.vault_token_account.to_account_info(),
        ra[R_VAULT_CTOKEN].clone(),
        ctx.accounts.token_program.to_account_info(),
        ra[R_INSTR_SYSVAR].clone(),
        ra[R_PROGRAM].clone(),
    ]
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
    /// CHECK: vault_authority PDA (token authority + protocol CPI signer); no data.
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump)]
    pub vault_authority: UncheckedAccount<'info>,
    pub base_mint: InterfaceAccount<'info, Mint>,
    /// Kamino reserve_collateral_mint (cToken mint for the USDC reserve).
    pub collateral_mint: InterfaceAccount<'info, Mint>,
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
        seeds = [VAULT_CTOKEN_SEED, position.key().as_ref()],
        bump,
        token::mint = collateral_mint,
        token::authority = vault_authority,
        token::token_program = token_program,
    )]
    pub vault_ctoken: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub token_program: Interface<'info, TokenInterface>,
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
    /// CHECK: vault_authority PDA (validated by seeds).
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump = position.vault_authority_bump)]
    pub vault_authority: UncheckedAccount<'info>,
    /// CHECK: base mint (USDC); validated == reserve.liquidity_mint.
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: vault USDC token account (PDA); validated in validate_vault. Mut: USDC moves in/out.
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    pub owner: Signer<'info>,
    /// CHECK: owner USDC token account; transfer authority is the owner.
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
    /// CHECK: vault_authority PDA (validated by seeds).
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump = position.vault_authority_bump)]
    pub vault_authority: UncheckedAccount<'info>,
    /// CHECK: base mint (USDC); validated == reserve.liquidity_mint.
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

    /// Live Kamino USDC reserve snapshot (read via scripts/inspect-kamino.ts). The expected
    /// values are computed independently in that script, so this pins our on-chain math to chain.
    fn snapshot() -> ReserveView {
        ReserveView {
            lending_market: Pubkey::default(),
            liquidity_mint: Pubkey::default(),
            liquidity_supply: Pubkey::default(),
            collateral_mint: Pubkey::default(),
            total_available_amount: 20_461_901_906_193,
            borrowed_amount_sf: 111_190_108_435_393_970_240_516_885_912_978,
            accumulated_protocol_fees_sf: 517_551_232_504_584_461_869_704_879_745,
            accumulated_referrer_fees_sf: 0,
            pending_referrer_fees_sf: 0,
            mint_total_supply: 98_179_501_119_636,
        }
    }

    #[test]
    fn ctoken_value_matches_onchain_snapshot() {
        let rv = snapshot();
        assert_eq!(ctoken_to_liquidity(1_000_000, &rv).unwrap(), 1_186_144);
        assert_eq!(ctoken_to_liquidity(25_000_000, &rv).unwrap(), 29_653_604);
    }

    #[test]
    fn ctoken_value_edge_cases() {
        let mut rv = snapshot();
        assert_eq!(ctoken_to_liquidity(0, &rv).unwrap(), 0);
        rv.mint_total_supply = 0;
        assert_eq!(ctoken_to_liquidity(123, &rv).unwrap(), 123); // INITIAL 1:1
    }

    #[test]
    fn mul_div_exact() {
        assert_eq!(mul_div_u64(6, 7, 3), Some(14));
        assert_eq!(mul_div_u64(0, u128::MAX, 1), Some(0));
        // (2^64-1) * (2^128-1) / (2^64-1) = 2^128-1
        assert_eq!(mul_div_u64(u64::MAX, u128::MAX, u64::MAX), Some(u128::MAX));
        assert_eq!(mul_div_u64(1, 1, 0), None);
    }
}
