// ============================================================
// Program:  ya-registry  (Yield Adapter Standard — registry)
// Framework: Anchor 1.0.2
// Testing:   LiteSVM (tests/registry_tests.rs)
// Risk Level: 🟡 Medium — holds admin keys (governance/guardian); custodies no funds.
// Security:  See programs/ya-registry/security-checklist.md
//
// Governance-gated approval of adapters. The dispatcher reads an AdapterEntry and refuses to
// route unless status == Active. Governance is a plain pubkey (swap in a Squads multisig with
// zero code changes). Governance rotation is TWO-STEP (propose/accept) per the admin-key rule.
// ============================================================
#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;

declare_id!("3ehQoDePP3eULnSKxgHc6DvLAwEQNeVHvJYzWPXoQyUD");

/// Singleton registry PDA seed.
pub const REGISTRY_SEED: &[u8] = b"registry";
/// Per-adapter entry PDA seed: `[ADAPTER_SEED, program_id]`.
pub const ADAPTER_SEED: &[u8] = b"adapter";
/// risk_tier is informational, 0..=3.
pub const MAX_RISK_TIER: u8 = 3;

#[program]
pub mod ya_registry {
    use super::*;

    /// Create the singleton registry. Callable once (the `init` constraint enforces it).
    pub fn initialize_registry(
        ctx: Context<InitializeRegistry>,
        governance: Pubkey,
        guardian: Pubkey,
    ) -> Result<()> {
        let registry = &mut ctx.accounts.registry;
        registry.governance = governance;
        registry.guardian = guardian;
        registry.pending_governance = None;
        registry.adapter_count = 0;
        // SECURITY: store the canonical bump; re-derive with it, never brute-force again.
        registry.bump = ctx.bumps.registry;

        emit!(RegistryInitialized { governance, guardian });
        Ok(())
    }

    /// Propose a new adapter. Restricted to governance (clean governance story; a permissionless
    /// variant is documented in SPEC). Creates an AdapterEntry in `Proposed` status.
    pub fn propose_adapter(
        ctx: Context<ProposeAdapter>,
        program_id: Pubkey,
        base_mint: Pubkey,
        name: String,
        version: u16,
        risk_tier: u8,
        remaining_accounts_hint: u8,
    ) -> Result<()> {
        // SECURITY: validate inputs before touching state.
        require!(name.len() <= 32, RegistryError::NameTooLong);
        require!(risk_tier <= MAX_RISK_TIER, RegistryError::InvalidRiskTier);

        let entry = &mut ctx.accounts.adapter_entry;
        entry.program_id = program_id;
        entry.base_mint = base_mint;
        entry.status = AdapterStatus::Proposed;
        entry.name = pad_name(&name);
        entry.version = version;
        entry.risk_tier = risk_tier;
        entry.remaining_accounts_hint = remaining_accounts_hint;
        entry.proposed_by = ctx.accounts.governance.key();
        entry.added_ts = Clock::get()?.unix_timestamp;
        entry.bump = ctx.bumps.adapter_entry;

        let registry = &mut ctx.accounts.registry;
        // SECURITY: checked arithmetic on the counter.
        registry.adapter_count = registry
            .adapter_count
            .checked_add(1)
            .ok_or(RegistryError::MathOverflow)?;

        emit!(AdapterProposed { program_id, base_mint, risk_tier });
        Ok(())
    }

    /// Governance approves a proposed adapter: Proposed -> Active.
    pub fn approve_adapter(ctx: Context<GovernedEntry>) -> Result<()> {
        let entry = &mut ctx.accounts.adapter_entry;
        require!(
            entry.status == AdapterStatus::Proposed,
            RegistryError::InvalidStatusTransition
        );
        set_status(entry, AdapterStatus::Active);
        Ok(())
    }

    /// Guardian OR governance pauses an active adapter: Active -> Paused (fast reaction).
    pub fn pause_adapter(ctx: Context<GuardedEntry>) -> Result<()> {
        // SECURITY: authority must be guardian or governance (explicit signer check).
        let authority = ctx.accounts.authority.key();
        let registry = &ctx.accounts.registry;
        require!(
            authority == registry.governance || authority == registry.guardian,
            RegistryError::Unauthorized
        );
        let entry = &mut ctx.accounts.adapter_entry;
        require!(
            entry.status == AdapterStatus::Active,
            RegistryError::InvalidStatusTransition
        );
        set_status(entry, AdapterStatus::Paused);
        Ok(())
    }

    /// Governance resumes a paused adapter: Paused -> Active.
    pub fn resume_adapter(ctx: Context<GovernedEntry>) -> Result<()> {
        let entry = &mut ctx.accounts.adapter_entry;
        require!(
            entry.status == AdapterStatus::Paused,
            RegistryError::InvalidStatusTransition
        );
        set_status(entry, AdapterStatus::Active);
        Ok(())
    }

    /// Governance deprecates an adapter: any -> Deprecated (terminal).
    pub fn deprecate_adapter(ctx: Context<GovernedEntry>) -> Result<()> {
        let entry = &mut ctx.accounts.adapter_entry;
        require!(
            entry.status != AdapterStatus::Deprecated,
            RegistryError::AlreadyDeprecated
        );
        set_status(entry, AdapterStatus::Deprecated);
        Ok(())
    }

    /// Step 1 of governance rotation: current governance proposes a successor.
    pub fn propose_governance(ctx: Context<Governed>, new_governance: Pubkey) -> Result<()> {
        ctx.accounts.registry.pending_governance = Some(new_governance);
        emit!(GovernanceProposed { new_governance });
        Ok(())
    }

    /// Step 2 of governance rotation: the proposed successor accepts (proves key control).
    pub fn accept_governance(ctx: Context<AcceptGovernance>) -> Result<()> {
        let registry = &mut ctx.accounts.registry;
        let pending = registry
            .pending_governance
            .ok_or(RegistryError::NoPendingGovernance)?;
        // SECURITY: only the proposed key can accept.
        require_keys_eq!(
            ctx.accounts.new_governance.key(),
            pending,
            RegistryError::NotPendingGovernance
        );
        registry.governance = pending;
        registry.pending_governance = None;
        emit!(GovernanceAccepted {
            governance: pending
        });
        Ok(())
    }

    /// Governance sets the guardian directly (guardian is a subordinate, pause-only key).
    pub fn set_guardian(ctx: Context<Governed>, new_guardian: Pubkey) -> Result<()> {
        ctx.accounts.registry.guardian = new_guardian;
        emit!(GuardianSet { guardian: new_guardian });
        Ok(())
    }
}

// ── helpers ─────────────────────────────────────────────────
fn pad_name(name: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    let bytes = name.as_bytes();
    out[..bytes.len()].copy_from_slice(bytes);
    out
}

fn set_status(entry: &mut AdapterEntry, new: AdapterStatus) {
    let old = entry.status;
    entry.status = new;
    emit!(AdapterStatusChanged {
        program_id: entry.program_id,
        old,
        new,
    });
}

// ── accounts ────────────────────────────────────────────────
#[derive(Accounts)]
pub struct InitializeRegistry<'info> {
    #[account(
        init,
        payer = payer,
        space = Registry::SPACE,
        seeds = [REGISTRY_SEED],
        bump,
    )]
    pub registry: Account<'info, Registry>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(program_id: Pubkey)]
pub struct ProposeAdapter<'info> {
    #[account(
        mut,
        seeds = [REGISTRY_SEED],
        bump = registry.bump,
        // SECURITY: only governance can propose (restricted variant).
        has_one = governance @ RegistryError::Unauthorized,
    )]
    pub registry: Account<'info, Registry>,
    #[account(
        init,
        payer = governance,
        space = AdapterEntry::SPACE,
        // SECURITY: PDA keyed by the adapter program id — one entry per program, no collisions.
        seeds = [ADAPTER_SEED, program_id.as_ref()],
        bump,
    )]
    pub adapter_entry: Account<'info, AdapterEntry>,
    #[account(mut)]
    pub governance: Signer<'info>,
    pub system_program: Program<'info, System>,
}

/// Governance-only action on an existing entry (approve/resume/deprecate).
#[derive(Accounts)]
pub struct GovernedEntry<'info> {
    #[account(
        seeds = [REGISTRY_SEED],
        bump = registry.bump,
        has_one = governance @ RegistryError::Unauthorized,
    )]
    pub registry: Account<'info, Registry>,
    #[account(
        mut,
        // SECURITY: re-derive the entry PDA from its STORED program_id with its STORED bump.
        seeds = [ADAPTER_SEED, adapter_entry.program_id.as_ref()],
        bump = adapter_entry.bump,
    )]
    pub adapter_entry: Account<'info, AdapterEntry>,
    pub governance: Signer<'info>,
}

/// Guardian-or-governance action on an existing entry (pause).
#[derive(Accounts)]
pub struct GuardedEntry<'info> {
    #[account(seeds = [REGISTRY_SEED], bump = registry.bump)]
    pub registry: Account<'info, Registry>,
    #[account(
        mut,
        seeds = [ADAPTER_SEED, adapter_entry.program_id.as_ref()],
        bump = adapter_entry.bump,
    )]
    pub adapter_entry: Account<'info, AdapterEntry>,
    pub authority: Signer<'info>,
}

/// Governance-only registry mutation (propose_governance / set_guardian).
#[derive(Accounts)]
pub struct Governed<'info> {
    #[account(
        mut,
        seeds = [REGISTRY_SEED],
        bump = registry.bump,
        has_one = governance @ RegistryError::Unauthorized,
    )]
    pub registry: Account<'info, Registry>,
    pub governance: Signer<'info>,
}

#[derive(Accounts)]
pub struct AcceptGovernance<'info> {
    #[account(mut, seeds = [REGISTRY_SEED], bump = registry.bump)]
    pub registry: Account<'info, Registry>,
    /// The proposed successor; must sign to prove key control.
    pub new_governance: Signer<'info>,
}

// ── state ───────────────────────────────────────────────────
#[account]
#[derive(Default)]
pub struct Registry {
    pub governance: Pubkey,
    pub guardian: Pubkey,
    pub pending_governance: Option<Pubkey>,
    pub adapter_count: u64,
    pub bump: u8,
}
impl Registry {
    // 8 disc + governance + guardian + Option<Pubkey>(1+32) + adapter_count + bump
    pub const SPACE: usize = 8 + 32 + 32 + (1 + 32) + 8 + 1;
}

#[account]
#[derive(Default)]
pub struct AdapterEntry {
    pub program_id: Pubkey,
    pub base_mint: Pubkey,
    pub status: AdapterStatus,
    pub name: [u8; 32],
    pub version: u16,
    pub risk_tier: u8,
    pub remaining_accounts_hint: u8,
    pub proposed_by: Pubkey,
    pub added_ts: i64,
    pub bump: u8,
}
impl AdapterEntry {
    // 8 disc + program_id + base_mint + status(1) + name(32) + version(2) + risk_tier(1)
    // + remaining_accounts_hint(1) + proposed_by(32) + added_ts(8) + bump(1)
    pub const SPACE: usize = 8 + 32 + 32 + 1 + 32 + 2 + 1 + 1 + 32 + 8 + 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AdapterStatus {
    #[default]
    Proposed,
    Active,
    Paused,
    Deprecated,
}

// ── events ──────────────────────────────────────────────────
#[event]
pub struct RegistryInitialized {
    pub governance: Pubkey,
    pub guardian: Pubkey,
}
#[event]
pub struct AdapterProposed {
    pub program_id: Pubkey,
    pub base_mint: Pubkey,
    pub risk_tier: u8,
}
#[event]
pub struct AdapterStatusChanged {
    pub program_id: Pubkey,
    pub old: AdapterStatus,
    pub new: AdapterStatus,
}
#[event]
pub struct GovernanceProposed {
    pub new_governance: Pubkey,
}
#[event]
pub struct GovernanceAccepted {
    pub governance: Pubkey,
}
#[event]
pub struct GuardianSet {
    pub guardian: Pubkey,
}

// ── errors ──────────────────────────────────────────────────
#[error_code]
pub enum RegistryError {
    #[msg("Caller is not authorized for this action")]
    Unauthorized,
    #[msg("Invalid adapter status transition")]
    InvalidStatusTransition,
    #[msg("Adapter is already deprecated (terminal)")]
    AlreadyDeprecated,
    #[msg("Adapter name exceeds 32 bytes")]
    NameTooLong,
    #[msg("risk_tier must be 0..=3")]
    InvalidRiskTier,
    #[msg("No pending governance to accept")]
    NoPendingGovernance,
    #[msg("Signer is not the pending governance")]
    NotPendingGovernance,
    #[msg("Arithmetic overflow")]
    MathOverflow,
}
