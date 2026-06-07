// ============================================================
// Program:  ya-dispatcher  (Yield Adapter Standard — router)
// Framework: Anchor 1.0.2
// Testing:   LiteSVM (tests/dispatcher_tests.rs)
// Risk Level: 🔴 Critical — routes value-bearing flows. NON-CUSTODIAL (router mode): it never
//             holds funds and uses `invoke` (not invoke_signed), so it cannot rug. Its security
//             rests on registry gating + program-id/base-mint checks.
// Security:  See programs/ya-dispatcher/security-checklist.md
//
// The dispatcher takes the standard 9-account prefix (§4.3) + the target adapter program +
// opaque remaining_accounts, asserts the registry entry is Active and matches, then forwards the
// identical standardized call to the adapter via one manual CPI. The end user signs the outer
// tx, so their signature propagates — no dispatcher PDA needed.
// ============================================================
#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::AccountMeta;
use ya_interface::{constants::ix, CpiCall, ValueReported, YaError};
use ya_registry::AdapterStatus;

declare_id!("2aY1hBVBJJmX8uSgB4aqhuS2xeDaGCc3d55KE2Mbvvgs");

#[program]
pub mod ya_dispatcher {
    use super::*;

    pub fn route_deposit<'info>(
        ctx: Context<'info, Route<'info>>,
        amount: u64,
        min_position_out: u64,
    ) -> Result<()> {
        gate(&ctx)?;
        CpiCall::global(ctx.accounts.adapter_program.key(), ix::DEPOSIT)
            .arg(&amount)
            .arg(&min_position_out)
            .metas(forward_metas(&ctx))
            .invoke(&forward_infos(&ctx))?;
        emit!(Routed {
            adapter: ctx.accounts.adapter_program.key(),
            position: ctx.accounts.position.key(),
            action: RouteAction::Deposit,
        });
        Ok(())
    }

    pub fn route_withdraw<'info>(
        ctx: Context<'info, Route<'info>>,
        shares: u64,
        min_amount_out: u64,
    ) -> Result<()> {
        gate(&ctx)?;
        CpiCall::global(ctx.accounts.adapter_program.key(), ix::WITHDRAW)
            .arg(&shares)
            .arg(&min_amount_out)
            .metas(forward_metas(&ctx))
            .invoke(&forward_infos(&ctx))?;
        emit!(Routed {
            adapter: ctx.accounts.adapter_program.key(),
            position: ctx.accounts.position.key(),
            action: RouteAction::Withdraw,
        });
        Ok(())
    }

    pub fn route_settle_withdrawal<'info>(
        ctx: Context<'info, Route<'info>>,
        min_amount_out: u64,
    ) -> Result<()> {
        gate(&ctx)?;
        CpiCall::global(ctx.accounts.adapter_program.key(), ix::SETTLE_WITHDRAWAL)
            .arg(&min_amount_out)
            .metas(forward_metas(&ctx))
            .invoke(&forward_infos(&ctx))?;
        emit!(Routed {
            adapter: ctx.accounts.adapter_program.key(),
            position: ctx.accounts.position.key(),
            action: RouteAction::SettleWithdrawal,
        });
        Ok(())
    }

    /// View: forwards `current_value`, reads the adapter's returned u64, re-emits + re-returns it
    /// so the dispatcher is itself view-callable (simulate + read returnData).
    pub fn route_current_value<'info>(ctx: Context<'info, Route<'info>>) -> Result<()> {
        gate(&ctx)?;
        CpiCall::global(ctx.accounts.adapter_program.key(), ix::CURRENT_VALUE)
            .metas(forward_metas(&ctx))
            .invoke(&forward_infos(&ctx))?;
        let value = ya_interface::read_returned_value(&ctx.accounts.adapter_program.key())
            .ok_or(YaError::OracleStale)?;
        ya_interface::report_value(value);
        emit!(ValueReported {
            position: ctx.accounts.position.key(),
            value,
        });
        Ok(())
    }
}

// ── gating ──────────────────────────────────────────────────
fn gate(ctx: &Context<Route>) -> Result<()> {
    // IDL-free load: verifies owner + discriminator + that this is the canonical entry PDA for
    // adapter_program (so program-id is bound here, not via a typed cross-crate account).
    let entry = ya_registry::load_adapter_entry(
        &ctx.accounts.registry_entry.to_account_info(),
        &ctx.accounts.adapter_program.key(),
    )?;
    require!(entry.status == AdapterStatus::Active, YaError::AdapterNotActive);
    require_keys_eq!(
        entry.base_mint,
        ctx.accounts.base_mint.key(),
        YaError::BaseMintMismatch
    );
    Ok(())
}

// ── account-meta / account-info forwarding (prefix §4.3 + opaque remaining) ──
fn forward_metas(ctx: &Context<Route>) -> Vec<AccountMeta> {
    let a = &ctx.accounts;
    let mut metas = vec![
        AccountMeta::new(a.position.key(), false),
        AccountMeta::new_readonly(a.vault_authority.key(), false),
        AccountMeta::new_readonly(a.base_mint.key(), false),
        AccountMeta::new(a.vault_token_account.key(), false),
        AccountMeta::new(a.owner.key(), true),
        AccountMeta::new(a.owner_token_account.key(), false),
        AccountMeta::new_readonly(a.registry_entry.key(), false),
        AccountMeta::new_readonly(a.token_program.key(), false),
        AccountMeta::new_readonly(a.system_program.key(), false),
    ];
    // remaining accounts are opaque to the dispatcher; preserve their signer/writable flags.
    for acc in ctx.remaining_accounts.iter() {
        metas.push(AccountMeta {
            pubkey: *acc.key,
            is_signer: acc.is_signer,
            is_writable: acc.is_writable,
        });
    }
    metas
}

fn forward_infos<'info>(ctx: &Context<'info, Route<'info>>) -> Vec<AccountInfo<'info>> {
    let a = &ctx.accounts;
    let mut infos = vec![
        a.position.to_account_info(),
        a.vault_authority.to_account_info(),
        a.base_mint.to_account_info(),
        a.vault_token_account.to_account_info(),
        a.owner.to_account_info(),
        a.owner_token_account.to_account_info(),
        a.registry_entry.to_account_info(),
        a.token_program.to_account_info(),
        a.system_program.to_account_info(),
    ];
    infos.extend(ctx.remaining_accounts.iter().cloned());
    // invoke requires the callee program account to be present in the infos.
    infos.push(a.adapter_program.to_account_info());
    infos
}

// ── accounts ────────────────────────────────────────────────
#[derive(Accounts)]
pub struct Route<'info> {
    /// CHECK: forwarded to the adapter (its position PDA); opaque to the router.
    #[account(mut)]
    pub position: UncheckedAccount<'info>,
    /// CHECK: forwarded (adapter vault_authority PDA).
    pub vault_authority: UncheckedAccount<'info>,
    /// CHECK: forwarded; validated == registry_entry.base_mint in `gate`.
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: forwarded (adapter vault token account).
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    /// The end user. Signs the outer tx; signature propagates into the adapter CPI.
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: forwarded (owner token account).
    #[account(mut)]
    pub owner_token_account: UncheckedAccount<'info>,
    /// CHECK: the registry entry for the target adapter. Validated in `gate` via
    /// ya_registry::load_adapter_entry (owner == registry, discriminator, and canonical
    /// `[b"adapter", adapter_program]` PDA) — no typed cross-crate account (keeps IDLs clean).
    pub registry_entry: UncheckedAccount<'info>,
    /// CHECK: forwarded (SPL/Token-2022 program).
    pub token_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: the target adapter program. Must be executable; its identity is bound to the
    /// (canonical) registry entry in `gate` — fully generic, no per-adapter special-casing.
    #[account(executable)]
    pub adapter_program: UncheckedAccount<'info>,
    // remaining_accounts: protocol-specific (incl. the withdrawal ticket for withdraw/settle),
    // forwarded opaquely to the adapter.
}

// ── events ──────────────────────────────────────────────────
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteAction {
    Deposit,
    Withdraw,
    SettleWithdrawal,
}

#[event]
pub struct Routed {
    pub adapter: Pubkey,
    pub position: Pubkey,
    pub action: RouteAction,
}
