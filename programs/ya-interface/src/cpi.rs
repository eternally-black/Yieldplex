//! The uniform manual-CPI primitive used by ALL adapters (no `declare_program!`).
//!
//! Every protocol call is built the same way:
//! ```ignore
//! CpiCall::global(KAMINO_PROGRAM, "deposit_reserve_liquidity")
//!     .arg(&amount)
//!     .account(reserve, false, true)
//!     // ... protocol accounts in IDL order ...
//!     .invoke_signed(account_infos, vault_authority_seeds)?;
//! ```
//! Discriminators are derived from the instruction name (no magic numbers); a unit test
//! cross-checks them against the vendored IDL bytes.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::{invoke, invoke_signed},
};
use sha2::{Digest, Sha256};

/// Anchor discriminator = first 8 bytes of sha256("<namespace>:<name>").
/// `namespace` is "global" for instructions and "account" for account types.
pub fn anchor_discriminator(namespace: &str, name: &str) -> [u8; 8] {
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    hasher.update(b":");
    hasher.update(name.as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    out
}

/// Fluent builder for one CPI into an external program. Keeps every adapter's protocol
/// calls byte-for-byte uniform and auditable.
pub struct CpiCall {
    program_id: Pubkey,
    accounts: Vec<AccountMeta>,
    data: Vec<u8>,
}

impl CpiCall {
    /// Start a call with an explicit 8-byte discriminator.
    pub fn new(program_id: Pubkey, discriminator: [u8; 8]) -> Self {
        Self {
            program_id,
            accounts: Vec::new(),
            data: discriminator.to_vec(),
        }
    }

    /// Start a call to a `global` (instruction) handler, deriving the discriminator from its name.
    pub fn global(program_id: Pubkey, ix_name: &str) -> Self {
        Self::new(program_id, anchor_discriminator("global", ix_name))
    }

    /// Append a Borsh-serialized argument (in declaration order).
    pub fn arg<T: AnchorSerialize>(mut self, arg: &T) -> Self {
        arg.serialize(&mut self.data).expect("borsh serialize cpi arg");
        self
    }

    /// Append raw bytes to the instruction data (escape hatch for non-Borsh layouts).
    pub fn raw(mut self, bytes: &[u8]) -> Self {
        self.data.extend_from_slice(bytes);
        self
    }

    /// Append one account meta (in the protocol's IDL account order).
    pub fn account(mut self, pubkey: Pubkey, is_signer: bool, is_writable: bool) -> Self {
        self.accounts.push(if is_writable {
            AccountMeta::new(pubkey, is_signer)
        } else {
            AccountMeta::new_readonly(pubkey, is_signer)
        });
        self
    }

    /// Append several account metas at once.
    pub fn metas(mut self, metas: impl IntoIterator<Item = AccountMeta>) -> Self {
        self.accounts.extend(metas);
        self
    }

    /// Materialize the `Instruction` (useful for tests / inspection).
    pub fn instruction(self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: self.accounts,
            data: self.data,
        }
    }

    /// Invoke without PDA signing (caller signature propagates).
    pub fn invoke(self, account_infos: &[AccountInfo]) -> Result<()> {
        invoke(&self.instruction(), account_infos).map_err(Into::into)
    }

    /// Invoke with PDA signer seeds (vault_authority signs protocol CPIs).
    pub fn invoke_signed(
        self,
        account_infos: &[AccountInfo],
        signer_seeds: &[&[&[u8]]],
    ) -> Result<()> {
        invoke_signed(&self.instruction(), account_infos, signer_seeds).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminators_match_vendored_idl_bytes() {
        // Cross-check derived discriminators against the exact bytes dumped from the IDLs
        // (idls/CPI-META.md). No magic numbers: the names are the source of truth.
        assert_eq!(
            anchor_discriminator("global", "deposit_reserve_liquidity"),
            [169, 201, 30, 126, 6, 205, 102, 68]
        );
        assert_eq!(
            anchor_discriminator("global", "lending_account_deposit"),
            [171, 94, 235, 103, 82, 64, 212, 140]
        );
        assert_eq!(
            anchor_discriminator("global", "add_insurance_fund_stake"),
            [251, 144, 115, 11, 222, 47, 62, 236]
        );
        assert_eq!(
            anchor_discriminator("global", "add_liquidity2"),
            [228, 162, 78, 28, 70, 219, 116, 115]
        );
    }
}
