// ============================================================
// Program:  ya-adapter-jupiter-jlp  (Yield Adapter Standard — Jupiter Perps JLP USDC adapter)
// Framework: Anchor 1.0.2
// Risk Level: 🔴 Critical — custodies JLP in the vault_authority PDA, CPIs USDC↔JLP via Jupiter
//             Perps add/remove_liquidity2, values the JLP position by pool NAV.
// Security:  See programs/ya-adapter-jupiter-jlp/security-checklist.md
//
// Reference adapter #3. Same shape as the Kamino/MarginFi adapters (standard prefix + vault model
// + uniform manual CPI), for Jupiter Perps JLP:
//   deposit  -> transfer owner USDC -> vault, then add_liquidity2(usdc, min_jlp, None) -> JLP. Instant.
//   withdraw -> remove_liquidity2(jlp, min_usdc) -> vault USDC, then transfer -> owner.
//   current_value -> jlp_balance * Pool.aum_usd / JLP_mint.supply (the pool NAV), via return data.
// add/remove_liquidity2 carry 14 protocol accounts (custody + its doves/pythnet oracles) — the tx is
// built with an Address Lookup Table (see tests). One CPI per op, invoke_signed by vault_authority.
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

declare_id!("9fqh4833yoSJoPzpsucHY2SbUafVfHcC48RLQhTTahsB");

ya_interface::declare_ya_accounts!(); // Position + WithdrawalTicket (owned by this program)

/// Jupiter Perps program + the JLP pool / mint (verified on-chain in M0).
const PERPS_ID: Pubkey = pubkey!("PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu");
const JLP_POOL: Pubkey = pubkey!("5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq");
const JLP_MINT: Pubkey = pubkey!("27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4");
const POOL_DISC: [u8; 8] = [241, 154, 109, 4, 17, 177, 109, 188];

const VAULT_USDC_SEED: &[u8] = b"vault_usdc";
const VAULT_JLP_SEED: &[u8] = b"vault_jlp";

// remaining_accounts for deposit / withdraw (after the prefix; withdraw prepends the ticket):
//   0 vault_jlp(w) · 1 transfer_authority · 2 perpetuals · 3 pool(w) · 4 custody(w)
//   5 custody_doves_price · 6 custody_pythnet_price · 7 custody_token_account(w)
//   8 lp_token_mint(w) · 9 event_authority · 10 perps_program
const J_VAULT_JLP: usize = 0;
const J_TRANSFER_AUTH: usize = 1;
const J_PERPETUALS: usize = 2;
const J_POOL: usize = 3;
const J_CUSTODY: usize = 4;
const J_DOVES: usize = 5;
const J_PYTHNET: usize = 6;
const J_CUSTODY_TOKEN: usize = 7;
const J_LP_MINT: usize = 8;
const J_EVENT_AUTH: usize = 9;
const J_PROGRAM: usize = 10;
const J_LEN: usize = 11;

#[program]
pub mod ya_adapter_jupiter_jlp {
    use super::*;

    /// Idempotent. Creates the Position, vault USDC account, and vault JLP account.
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

    /// Deposit USDC into the JLP pool, receiving JLP into the vault.
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
        require!(ra.len() >= J_LEN, YaError::InvalidRemainingAccounts);
        validate_jlp(ra)?;
        validate_vault(
            &ctx.accounts.vault_token_account.key(),
            &ra[J_VAULT_JLP],
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

        // 2) vault adds liquidity -> JLP. args = AddLiquidity2Params{token_amount_in, min_lp_amount_out, None}.
        let before = token_amount(&ra[J_VAULT_JLP])?;
        let position_key = ctx.accounts.position.key();
        let va_bump = [ctx.accounts.position.vault_authority_bump];
        let signer: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, position_key.as_ref(), &va_bump]];
        CpiCall::global(PERPS_ID, "add_liquidity2")
            .arg(&amount)
            .arg(&min_position_out)
            .arg(&None::<u64>)
            .account(ctx.accounts.vault_authority.key(), true, false) // owner
            .account(ctx.accounts.vault_token_account.key(), false, true) // funding_account
            .account(ra[J_VAULT_JLP].key(), false, true) // lp_token_account
            .account(ra[J_TRANSFER_AUTH].key(), false, false)
            .account(ra[J_PERPETUALS].key(), false, false)
            .account(ra[J_POOL].key(), false, true)
            .account(ra[J_CUSTODY].key(), false, true)
            .account(ra[J_DOVES].key(), false, false)
            .account(ra[J_PYTHNET].key(), false, false)
            .account(ra[J_CUSTODY_TOKEN].key(), false, true)
            .account(ra[J_LP_MINT].key(), false, true)
            .account(ctx.accounts.token_program.key(), false, false)
            .account(ra[J_EVENT_AUTH].key(), false, false)
            .account(ra[J_PROGRAM].key(), false, false)
            // Jupiter revalues the whole pool: append all pool custodies + their oracles (ra[J_LEN..]).
            .metas(trailing_metas(ra))
            .invoke_signed(&jlp_infos(&ctx, ra), signer)?;

        let received = token_amount(&ra[J_VAULT_JLP])?
            .checked_sub(before)
            .ok_or(YaError::MathOverflow)?;
        require!(received >= min_position_out, YaError::SlippageExceeded);
        let supply = mint_supply(&ra[J_LP_MINT])?;
        let aum = read_pool_aum(&ra[J_POOL])?;
        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_add(received).ok_or(YaError::MathOverflow)?;
        p.cached_value = nav_value(p.shares, aum, supply)?;
        p.value_updated_ts = Clock::get()?.unix_timestamp;
        emit!(Deposited {
            position: p.key(),
            amount,
            value_after: p.cached_value,
        });
        Ok(())
    }

    /// Instant withdraw: remove liquidity for USDC, pay the owner, settle the ticket in one call.
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
        require!(ra.len() >= J_LEN, YaError::InvalidRemainingAccounts);
        require!(shares <= ctx.accounts.position.shares, YaError::SlippageExceeded);
        validate_jlp(ra)?;
        validate_vault(
            &ctx.accounts.vault_token_account.key(),
            &ra[J_VAULT_JLP],
            &ctx.accounts.position.key(),
        )?;

        let position_key = ctx.accounts.position.key();
        let va_bump = [ctx.accounts.position.vault_authority_bump];
        let signer: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, position_key.as_ref(), &va_bump]];

        let usdc_before = token_amount(&ctx.accounts.vault_token_account.to_account_info())?;
        CpiCall::global(PERPS_ID, "remove_liquidity2")
            .arg(&shares)
            .arg(&min_amount_out)
            .account(ctx.accounts.vault_authority.key(), true, false) // owner
            .account(ctx.accounts.vault_token_account.key(), false, true) // receiving_account
            .account(ra[J_VAULT_JLP].key(), false, true) // lp_token_account
            .account(ra[J_TRANSFER_AUTH].key(), false, false)
            .account(ra[J_PERPETUALS].key(), false, false)
            .account(ra[J_POOL].key(), false, true)
            .account(ra[J_CUSTODY].key(), false, true)
            .account(ra[J_DOVES].key(), false, false)
            .account(ra[J_PYTHNET].key(), false, false)
            .account(ra[J_CUSTODY_TOKEN].key(), false, true)
            .account(ra[J_LP_MINT].key(), false, true)
            .account(ctx.accounts.token_program.key(), false, false)
            .account(ra[J_EVENT_AUTH].key(), false, false)
            .account(ra[J_PROGRAM].key(), false, false)
            .metas(trailing_metas(ra))
            .invoke_signed(&jlp_infos_w(&ctx, ra), signer)?;
        let redeemed = token_amount(&ctx.accounts.vault_token_account.to_account_info())?
            .checked_sub(usdc_before)
            .ok_or(YaError::MathOverflow)?;

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

        let supply = mint_supply(&ra[J_LP_MINT])?;
        let aum = read_pool_aum(&ra[J_POOL])?;
        let p = &mut ctx.accounts.position;
        p.shares = p.shares.checked_sub(shares).ok_or(YaError::MathOverflow)?;
        p.cached_value = nav_value(p.shares, aum, supply)?;
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

    /// View: JLP NAV = jlp_balance * Pool.aum_usd / JLP_mint.supply.
    pub fn current_value<'info>(ctx: Context<'info, Op<'info>>) -> Result<()> {
        assert_active(
            &ctx.accounts.registry_entry.to_account_info(),
            &ctx.accounts.base_mint.key(),
        )?;
        let ra = ctx.remaining_accounts;
        require!(ra.len() >= 2, YaError::InvalidRemainingAccounts);
        require_keys_eq!(ra[0].key(), JLP_POOL, YaError::InvalidRemainingAccounts);
        require_keys_eq!(ra[1].key(), JLP_MINT, YaError::InvalidRemainingAccounts);
        let aum = read_pool_aum(&ra[0])?;
        let supply = mint_supply(&ra[1])?;

        let value = nav_value(ctx.accounts.position.shares, aum, supply)?;
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

// ── JLP value (NAV) ─────────────────────────────────────────
/// Pool.aum_usd (USD * 1e6), parsed dynamically: the Pool is a Borsh account with a leading
/// String (name) + Vec<pubkey> (custodies), so aum_usd's offset depends on both lengths.
fn read_pool_aum(ai: &AccountInfo) -> Result<u128> {
    require_keys_eq!(*ai.owner, PERPS_ID, YaError::InvalidRemainingAccounts);
    let data = ai.try_borrow_data()?;
    require!(data.len() >= 16 && data[0..8] == POOL_DISC, YaError::InvalidRemainingAccounts);
    let name_len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let c_off = 12usize.checked_add(name_len).ok_or(YaError::MathOverflow)?;
    require!(data.len() >= c_off + 4, YaError::InvalidRemainingAccounts);
    let n_cust = u32::from_le_bytes(data[c_off..c_off + 4].try_into().unwrap()) as usize;
    let aum_off = c_off
        .checked_add(4)
        .and_then(|x| x.checked_add(n_cust.checked_mul(32)?))
        .ok_or(YaError::MathOverflow)?;
    require!(data.len() >= aum_off + 16, YaError::InvalidRemainingAccounts);
    Ok(u128::from_le_bytes(data[aum_off..aum_off + 16].try_into().unwrap()))
}

/// JLP NAV in USDC base units = floor(shares * aum_usd / supply), narrowed to u64.
fn nav_value(shares: u64, aum_usd: u128, supply: u64) -> Result<u64> {
    let v = mul_div_u64(shares, aum_usd, supply).ok_or(YaError::MathOverflow)?;
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

// ── validation / token helpers ──────────────────────────────
fn assert_active(registry_entry: &AccountInfo, base_mint: &Pubkey) -> Result<()> {
    let entry = ya_registry::load_adapter_entry(registry_entry, &crate::ID)?;
    require!(entry.status == AdapterStatus::Active, YaError::AdapterNotActive);
    require_keys_eq!(entry.base_mint, *base_mint, YaError::BaseMintMismatch);
    Ok(())
}

fn validate_jlp(ra: &[AccountInfo]) -> Result<()> {
    require_keys_eq!(ra[J_POOL].key(), JLP_POOL, YaError::InvalidRemainingAccounts);
    require_keys_eq!(ra[J_LP_MINT].key(), JLP_MINT, YaError::InvalidRemainingAccounts);
    require_keys_eq!(ra[J_PROGRAM].key(), PERPS_ID, YaError::InvalidRemainingAccounts);
    Ok(())
}

fn validate_vault(vault_usdc: &Pubkey, vault_jlp: &AccountInfo, position: &Pubkey) -> Result<()> {
    let usdc_pda = Pubkey::find_program_address(&[VAULT_USDC_SEED, position.as_ref()], &crate::ID).0;
    require_keys_eq!(*vault_usdc, usdc_pda, YaError::InvalidRemainingAccounts);
    let jlp_pda = Pubkey::find_program_address(&[VAULT_JLP_SEED, position.as_ref()], &crate::ID).0;
    require_keys_eq!(*vault_jlp.key, jlp_pda, YaError::InvalidRemainingAccounts);
    Ok(())
}

fn token_amount(ai: &AccountInfo) -> Result<u64> {
    let data = ai.try_borrow_data()?;
    require!(data.len() >= 72, YaError::InvalidRemainingAccounts);
    Ok(u64::from_le_bytes(data[64..72].try_into().unwrap()))
}

fn mint_supply(ai: &AccountInfo) -> Result<u64> {
    let data = ai.try_borrow_data()?;
    require!(data.len() >= 44, YaError::InvalidRemainingAccounts);
    Ok(u64::from_le_bytes(data[36..44].try_into().unwrap()))
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

/// Pool-revaluation tail: all extra remaining accounts (the 5 custodies + 5 doves_ag oracles)
/// forwarded read-only after the 14 named add/remove_liquidity2 accounts.
fn trailing_metas(ra: &[AccountInfo]) -> Vec<AccountMeta> {
    ra[J_LEN..]
        .iter()
        .map(|a| AccountMeta::new_readonly(*a.key, false))
        .collect()
}

fn jlp_infos<'info>(ctx: &Context<'info, Op<'info>>, ra: &[AccountInfo<'info>]) -> Vec<AccountInfo<'info>> {
    let mut v = vec![
        ctx.accounts.vault_authority.to_account_info(),
        ctx.accounts.vault_token_account.to_account_info(),
        ctx.accounts.token_program.to_account_info(),
    ];
    v.extend(ra.iter().cloned());
    v
}

fn jlp_infos_w<'info>(
    ctx: &Context<'info, WithdrawOp<'info>>,
    ra: &[AccountInfo<'info>],
) -> Vec<AccountInfo<'info>> {
    let mut v = vec![
        ctx.accounts.vault_authority.to_account_info(),
        ctx.accounts.vault_token_account.to_account_info(),
        ctx.accounts.token_program.to_account_info(),
    ];
    v.extend(ra.iter().cloned());
    v
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
    /// JLP mint (the vault JLP account holds this).
    pub jlp_mint: InterfaceAccount<'info, Mint>,
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
        seeds = [VAULT_JLP_SEED, position.key().as_ref()],
        bump,
        token::mint = jlp_mint,
        token::authority = vault_authority,
        token::token_program = token_program,
    )]
    pub vault_jlp: InterfaceAccount<'info, TokenAccount>,
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
    fn jlp_nav_matches_onchain_snapshot() {
        // aum_usd=741476340987616, supply=220941229178279 -> price ~3.356; 5 JLP -> 16779945 (USDC 6dp).
        assert_eq!(mul_div_u64(5_000_000, 741_476_340_987_616, 220_941_229_178_279), Some(16_779_945));
        assert_eq!(mul_div_u64(0, 741_476_340_987_616, 220_941_229_178_279), Some(0));
        assert_eq!(mul_div_u64(1, 1, 0), None);
    }
}
