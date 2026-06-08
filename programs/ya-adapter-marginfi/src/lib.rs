// ============================================================
// Program:  ya-adapter-marginfi  (Yield Adapter Standard — MarginFi v2 USDC adapter)
// Framework: Anchor 1.0.2
// Risk Level: 🔴 Critical — custodies the position (a marginfi_account owned by the vault_authority
//             PDA), CPIs USDC in/out of MarginFi v2, computes redeemable value from share math.
// Security:  See programs/ya-adapter-marginfi/security-checklist.md
//
// Reference adapter #2. Same shape as ya-adapter-kamino (standard prefix + declare_ya_accounts!() +
// assert_active + vault model), swapping Kamino for MarginFi v2:
//   initialize_position -> creates the Position, vault USDC account, and a marginfi_account PDA
//                          (via marginfi_account_initialize, authority = vault_authority).
//   deposit  -> transfer owner USDC -> vault, then lending_account_deposit(amount). Instant.
//   withdraw -> lending_account_withdraw(amount, withdraw_all) -> vault, then transfer -> owner.
//   current_value -> reads our marginfi_account's asset_shares for the USDC bank * the bank's
//                    asset_share_value (I80F48) -> redeemable USDC, via return data.
// One manual CPI per op, invoke_signed by the per-position vault_authority. Deposit/withdraw
// accrue interest internally in-slot (no separate crank). value uses share math only.
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

declare_id!("36CgQYZFxZQHzyMrn3NJRXR9jsVoYH44WitqGohoBGoi");

ya_interface::declare_ya_accounts!(); // Position + WithdrawalTicket (owned by this program)

/// MarginFi v2 program (verified on-chain in M0).
const MARGINFI_ID: Pubkey = pubkey!("MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA");
const BANK_DISC: [u8; 8] = [142, 49, 166, 242, 50, 66, 97, 188];
const MFI_ACCOUNT_DISC: [u8; 8] = [67, 178, 130, 109, 126, 114, 28, 42];
/// Bank account length we touch (last read is liquidity_vault_authority_bump @145).
const BANK_MIN_LEN: usize = 146;
/// MarginfiAccount length we touch (lending_account balances[16] end at 72 + 16*104).
const MFI_ACCOUNT_MIN_LEN: usize = 72 + 16 * 104;

/// Vault PDA seeds (this adapter owns these).
const VAULT_USDC_SEED: &[u8] = b"vault_usdc";
const MARGINFI_ACCOUNT_SEED: &[u8] = b"marginfi_account";

// remaining_accounts layouts (after the standard prefix; withdraw prepends the ticket):
//   deposit : 0 marginfi_account(w) · 1 marginfi_group · 2 bank(w) · 3 bank_liquidity_vault(w) · 4 marginfi_program
const D_MFI_ACCOUNT: usize = 0;
const D_GROUP: usize = 1;
const D_BANK: usize = 2;
const D_LIQ_VAULT: usize = 3;
const D_PROGRAM: usize = 4;
const D_LEN: usize = 5;
//   withdraw: 0 marginfi_account(w) · 1 marginfi_group · 2 bank(w) · 3 bank_liquidity_vault_authority(w)
//             · 4 bank_liquidity_vault(w) · 5 oracle · 6 marginfi_program
const W_MFI_ACCOUNT: usize = 0;
const W_GROUP: usize = 1;
const W_BANK: usize = 2;
const W_LIQ_VAULT_AUTH: usize = 3;
const W_LIQ_VAULT: usize = 4;
const W_ORACLE: usize = 5;
const W_PROGRAM: usize = 6;
const W_LEN: usize = 7;

#[program]
pub mod ya_adapter_marginfi {
    use super::*;

    /// Idempotent. Creates the Position, vault USDC account, and (once) the marginfi_account PDA.
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

        // Create the marginfi_account once (authority = vault_authority). Guarded for idempotency.
        if ctx.accounts.marginfi_account.data_is_empty() {
            require_keys_eq!(
                ctx.accounts.marginfi_program.key(),
                MARGINFI_ID,
                YaError::InvalidRemainingAccounts
            );
            let position_key = ctx.accounts.position.key();
            let mfi_bump = [ctx.bumps.marginfi_account];
            let va_bump = [ctx.accounts.position.vault_authority_bump];
            let signer: &[&[&[u8]]] = &[
                &[MARGINFI_ACCOUNT_SEED, position_key.as_ref(), &mfi_bump],
                &[seeds::VAULT_AUTHORITY, position_key.as_ref(), &va_bump],
            ];
            CpiCall::global(MARGINFI_ID, "marginfi_account_initialize")
                .account(ctx.accounts.marginfi_group.key(), false, false)
                .account(ctx.accounts.marginfi_account.key(), true, true)
                .account(ctx.accounts.vault_authority.key(), true, false) // authority
                .account(ctx.accounts.owner.key(), true, true) // fee_payer
                .account(ctx.accounts.system_program.key(), false, false)
                .invoke_signed(
                    &[
                        ctx.accounts.marginfi_group.to_account_info(),
                        ctx.accounts.marginfi_account.to_account_info(),
                        ctx.accounts.vault_authority.to_account_info(),
                        ctx.accounts.owner.to_account_info(),
                        ctx.accounts.system_program.to_account_info(),
                        ctx.accounts.marginfi_program.to_account_info(),
                    ],
                    signer,
                )?;
        }
        Ok(())
    }

    /// Deposit USDC into the MarginFi USDC bank (credits asset_shares to our marginfi_account).
    pub fn deposit<'info>(
        ctx: Context<'info, Op<'info>>,
        amount: u64,
        min_position_out: u64,
    ) -> Result<()> {
        assert_active(
            &ctx.accounts.registry_entry.to_account_info(),
            &ctx.accounts.base_mint.key(),
        )?;
        // MarginFi deposits exactly `amount` tokens; position units == principal == amount.
        require!(amount >= min_position_out, YaError::SlippageExceeded);
        let ra = ctx.remaining_accounts;
        require!(ra.len() >= D_LEN, YaError::InvalidRemainingAccounts);
        let bank = read_bank(&ra[D_BANK])?;
        validate_marginfi(&bank, &ctx.accounts.base_mint.key(), &ra[D_GROUP], &ra[D_PROGRAM])?;
        validate_vault(
            &ctx.accounts.vault_token_account.key(),
            &ra[D_MFI_ACCOUNT],
            &ctx.accounts.position.key(),
        )?;

        // 1) user USDC -> vault USDC (owner signs).
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

        // 2) vault deposits into the bank.
        let position_key = ctx.accounts.position.key();
        let va_bump = [ctx.accounts.position.vault_authority_bump];
        let signer: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, position_key.as_ref(), &va_bump]];
        // Deployed program takes (amount, deposit_up_to_limit: Option<bool>); pass None.
        CpiCall::global(MARGINFI_ID, "lending_account_deposit")
            .arg(&amount)
            .arg(&None::<bool>)
            .account(ra[D_GROUP].key(), false, false)
            .account(ra[D_MFI_ACCOUNT].key(), false, true)
            .account(ctx.accounts.vault_authority.key(), true, false) // signer == authority
            .account(ra[D_BANK].key(), false, true)
            .account(ctx.accounts.vault_token_account.key(), false, true) // signer_token_account
            .account(ra[D_LIQ_VAULT].key(), false, true)
            .account(ctx.accounts.token_program.key(), false, false)
            .invoke_signed(
                &[
                    ra[D_GROUP].clone(),
                    ra[D_MFI_ACCOUNT].clone(),
                    ctx.accounts.vault_authority.to_account_info(),
                    ra[D_BANK].clone(),
                    ctx.accounts.vault_token_account.to_account_info(),
                    ra[D_LIQ_VAULT].clone(),
                    ctx.accounts.token_program.to_account_info(),
                    ra[D_PROGRAM].clone(),
                ],
                signer,
            )?;

        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_add(amount).ok_or(YaError::MathOverflow)?;
        let shares = marginfi_asset_shares(&ra[D_MFI_ACCOUNT], &ra[D_BANK].key())?;
        p.cached_value = shares_to_tokens(shares, bank.asset_share_value)?;
        p.value_updated_ts = Clock::get()?.unix_timestamp;
        emit!(Deposited {
            position: p.key(),
            amount,
            value_after: p.cached_value,
        });
        Ok(())
    }

    /// Instant withdraw: redeem shares for USDC, pay the owner, settle the ticket in one call.
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
        require!(ra.len() >= W_LEN, YaError::InvalidRemainingAccounts);
        require!(shares <= ctx.accounts.position.shares, YaError::SlippageExceeded);
        let bank = read_bank(&ra[W_BANK])?;
        validate_marginfi(&bank, &ctx.accounts.base_mint.key(), &ra[W_GROUP], &ra[W_PROGRAM])?;
        validate_vault(
            &ctx.accounts.vault_token_account.key(),
            &ra[W_MFI_ACCOUNT],
            &ctx.accounts.position.key(),
        )?;

        let withdraw_all = shares >= ctx.accounts.position.shares;
        let position_key = ctx.accounts.position.key();
        let va_bump = [ctx.accounts.position.vault_authority_bump];
        let signer: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, position_key.as_ref(), &va_bump]];

        // 1) bank -> vault USDC. remaining tail [bank, oracle] = the post-withdraw health check.
        let usdc_before = token_amount(&ctx.accounts.vault_token_account.to_account_info())?;
        CpiCall::global(MARGINFI_ID, "lending_account_withdraw")
            .arg(&shares)
            .arg(&Some(withdraw_all))
            .account(ra[W_GROUP].key(), false, false)
            .account(ra[W_MFI_ACCOUNT].key(), false, true)
            .account(ctx.accounts.vault_authority.key(), true, false) // signer == authority
            .account(ra[W_BANK].key(), false, true)
            .account(ctx.accounts.vault_token_account.key(), false, true) // destination
            .account(ra[W_LIQ_VAULT_AUTH].key(), false, true)
            .account(ra[W_LIQ_VAULT].key(), false, true)
            .account(ctx.accounts.token_program.key(), false, false)
            .account(ra[W_BANK].key(), false, false) // health: bank
            .account(ra[W_ORACLE].key(), false, false) // health: oracle
            .invoke_signed(
                &[
                    ra[W_GROUP].clone(),
                    ra[W_MFI_ACCOUNT].clone(),
                    ctx.accounts.vault_authority.to_account_info(),
                    ra[W_BANK].clone(),
                    ctx.accounts.vault_token_account.to_account_info(),
                    ra[W_LIQ_VAULT_AUTH].clone(),
                    ra[W_LIQ_VAULT].clone(),
                    ctx.accounts.token_program.to_account_info(),
                    ra[W_ORACLE].clone(),
                    ra[W_PROGRAM].clone(),
                ],
                signer,
            )?;
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

        let p = &mut ctx.accounts.position;
        p.shares = if withdraw_all {
            0
        } else {
            p.shares.checked_sub(shares).ok_or(YaError::MathOverflow)?
        };
        let live = marginfi_asset_shares(&ra[W_MFI_ACCOUNT], &ra[W_BANK].key())?;
        p.cached_value = shares_to_tokens(live, bank.asset_share_value)?;
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

    /// Instant adapter: nothing to settle.
    pub fn settle_withdrawal(_ctx: Context<Op>, _min_amount_out: u64) -> Result<()> {
        Err(YaError::NothingToSettle.into())
    }

    /// View: redeemable USDC = our asset_shares for the bank * the bank's asset_share_value.
    pub fn current_value<'info>(ctx: Context<'info, Op<'info>>) -> Result<()> {
        assert_active(
            &ctx.accounts.registry_entry.to_account_info(),
            &ctx.accounts.base_mint.key(),
        )?;
        let ra = ctx.remaining_accounts;
        require!(ra.len() >= 2, YaError::InvalidRemainingAccounts);
        let mfi_account = &ra[0];
        let bank = read_bank(&ra[1])?;
        require_keys_eq!(bank.mint, ctx.accounts.base_mint.key(), YaError::BaseMintMismatch);
        validate_vault(
            &ctx.accounts.vault_token_account.key(),
            mfi_account,
            &ctx.accounts.position.key(),
        )?;

        let shares = marginfi_asset_shares(mfi_account, &ra[1].key())?;
        let value = shares_to_tokens(shares, bank.asset_share_value)?;
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

// ── MarginFi value math (matches lending_account_withdraw payout) ────────────
struct BankView {
    group: Pubkey,
    mint: Pubkey,
    asset_share_value: u128, // WrappedI80F48 bits (token/share * 2^48)
    liquidity_vault: Pubkey,
}

/// W011: owner + discriminator + length checked before reading offsets.
fn read_bank(ai: &AccountInfo) -> Result<BankView> {
    require_keys_eq!(*ai.owner, MARGINFI_ID, YaError::InvalidRemainingAccounts);
    let data = ai.try_borrow_data()?;
    require!(
        data.len() >= BANK_MIN_LEN && data[0..8] == BANK_DISC,
        YaError::InvalidRemainingAccounts
    );
    Ok(BankView {
        mint: Pubkey::new_from_array(data[8..40].try_into().unwrap()),
        group: Pubkey::new_from_array(data[41..73].try_into().unwrap()),
        asset_share_value: u128::from_le_bytes(data[80..96].try_into().unwrap()),
        liquidity_vault: Pubkey::new_from_array(data[112..144].try_into().unwrap()),
    })
}

/// Our asset_shares (I80F48 bits) for `bank` from our marginfi_account's lending_account balances.
fn marginfi_asset_shares(ai: &AccountInfo, bank: &Pubkey) -> Result<u128> {
    require_keys_eq!(*ai.owner, MARGINFI_ID, YaError::InvalidRemainingAccounts);
    let data = ai.try_borrow_data()?;
    require!(
        data.len() >= MFI_ACCOUNT_MIN_LEN && data[0..8] == MFI_ACCOUNT_DISC,
        YaError::InvalidRemainingAccounts
    );
    for i in 0..16usize {
        let base = 72 + i * 104;
        if data[base] == 0 {
            continue; // inactive slot
        }
        let bank_pk = Pubkey::new_from_array(data[base + 1..base + 33].try_into().unwrap());
        if bank_pk == *bank {
            return Ok(u128::from_le_bytes(data[base + 40..base + 56].try_into().unwrap()));
        }
    }
    Ok(0)
}

/// floor(asset_shares * asset_share_value) = (a_bits * b_bits) >> 96, both WrappedI80F48 (x2^48).
/// Matches marginfi's `get_asset_amount(shares).checked_floor()` (truncating I80F48 mul).
fn shares_to_tokens(asset_shares_bits: u128, asset_share_value_bits: u128) -> Result<u64> {
    let (hi, lo) = mul_u128(asset_shares_bits, asset_share_value_bits);
    let value = hi
        .checked_mul(1u128 << 32) // hi * 2^(128-96)
        .and_then(|h| h.checked_add(lo >> 96))
        .ok_or(YaError::MathOverflow)?;
    require!(value <= u64::MAX as u128, YaError::MathOverflow);
    Ok(value as u64)
}

/// Full 256-bit product of two u128 -> (high 128, low 128). Panic-free, dependency-free.
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

// ── validation / token helpers ──────────────────────────────
fn assert_active(registry_entry: &AccountInfo, base_mint: &Pubkey) -> Result<()> {
    let entry = ya_registry::load_adapter_entry(registry_entry, &crate::ID)?;
    require!(entry.status == AdapterStatus::Active, YaError::AdapterNotActive);
    require_keys_eq!(entry.base_mint, *base_mint, YaError::BaseMintMismatch);
    Ok(())
}

/// §9.3: bind the bank to base_mint + group + the marginfi program passed in.
fn validate_marginfi(
    bank: &BankView,
    base_mint: &Pubkey,
    group: &AccountInfo,
    program: &AccountInfo,
) -> Result<()> {
    require_keys_eq!(bank.mint, *base_mint, YaError::BaseMintMismatch);
    require_keys_eq!(bank.group, *group.key, YaError::InvalidRemainingAccounts);
    require_keys_eq!(*program.key, MARGINFI_ID, YaError::InvalidRemainingAccounts);
    let _ = bank.liquidity_vault; // (forwarded; marginfi re-derives + checks its own vault PDAs)
    Ok(())
}

/// vault USDC + marginfi_account are this position's canonical PDAs (and marginfi-owned).
fn validate_vault(vault_usdc: &Pubkey, mfi_account: &AccountInfo, position: &Pubkey) -> Result<()> {
    let usdc_pda = Pubkey::find_program_address(&[VAULT_USDC_SEED, position.as_ref()], &crate::ID).0;
    require_keys_eq!(*vault_usdc, usdc_pda, YaError::InvalidRemainingAccounts);
    let mfi_pda =
        Pubkey::find_program_address(&[MARGINFI_ACCOUNT_SEED, position.as_ref()], &crate::ID).0;
    require_keys_eq!(*mfi_account.key, mfi_pda, YaError::InvalidRemainingAccounts);
    require_keys_eq!(*mfi_account.owner, MARGINFI_ID, YaError::InvalidRemainingAccounts);
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
    /// CHECK: vault_authority PDA (token authority, protocol CPI signer, marginfi_account authority).
    #[account(seeds = [seeds::VAULT_AUTHORITY, position.key().as_ref()], bump)]
    pub vault_authority: UncheckedAccount<'info>,
    pub base_mint: InterfaceAccount<'info, Mint>,
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
    /// CHECK: marginfi_account PDA — created by marginfi_account_initialize (CPI) on first call.
    #[account(mut, seeds = [MARGINFI_ACCOUNT_SEED, position.key().as_ref()], bump)]
    pub marginfi_account: UncheckedAccount<'info>,
    /// CHECK: the MarginFi group (validated == bank.group on deposit/withdraw).
    pub marginfi_group: UncheckedAccount<'info>,
    /// CHECK: MarginFi program; address-checked.
    #[account(address = MARGINFI_ID)]
    pub marginfi_program: UncheckedAccount<'info>,
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
    /// CHECK: base mint (USDC); validated == bank.mint.
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
    /// CHECK: base mint (USDC); validated == bank.mint.
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
    fn shares_to_tokens_matches_onchain_snapshot() {
        // 1:1 bank (asset_share_value = 2^48): 25e6 USDC worth of shares -> 25e6.
        assert_eq!(
            shares_to_tokens(7_036_874_417_766_400_000_000, 281_474_976_710_656).unwrap(),
            25_000_000
        );
        // Live main USDC bank (asset_share_value ~1.225): independently computed in inspect-marginfi.ts.
        assert_eq!(
            shares_to_tokens(5_743_961_595_447_965_917_543, 344_832_400_067_977).unwrap(),
            24_999_999
        );
    }

    #[test]
    fn shares_to_tokens_edges() {
        assert_eq!(shares_to_tokens(0, 344_832_400_067_977).unwrap(), 0);
        // 2^48 shares (== 1.0 in I80F48) * (2^48) value == 1 token.
        assert_eq!(shares_to_tokens(1u128 << 48, 1u128 << 48).unwrap(), 1);
    }

    #[test]
    fn mul_u128_full_product() {
        let (hi, lo) = mul_u128(u128::MAX, u128::MAX);
        // (2^128-1)^2 = 2^256 - 2^129 + 1 => hi = 2^128-2, lo = 1
        assert_eq!(hi, u128::MAX - 1);
        assert_eq!(lo, 1);
    }
}
