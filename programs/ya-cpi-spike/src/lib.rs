//! M0 spike result — the CPI integration approach (§9), proven empirically.
//!
//! FINDING: Anchor 1.0.2 `declare_program!` requires `extern crate anchor_lang;` at the
//! crate root (its generated submodules do `use super::anchor_lang`).
//!
//! With that, declare_program! generates clean, compilable CPI clients for:
//!   - kamino_lend   (Kamino adapter)
//!   - jupiter_perps (JLP adapter — uses add_liquidity2/remove_liquidity2)
//!   - syrup_swap_pool = Orca Whirlpool (Maple swap adapter — swap/swap_v2)
//!
//! It does NOT compile for `marginfi` (WrappedI80F48 not Borsh, zero-copy structs not Pod,
//! bytemuck out of scope — 37 errors) or `drift` (duplicate `padding` fields, array/type
//! mismatches — 5 errors). Those two protocols' zero-copy/complex IDLs break the codegen.
//! => marginfi + drift adapters use MANUAL CPI (discriminator + borsh(args) + AccountMetas)
//!    and targeted manual field deserialization for current_value. Discriminators + account
//!    orders come from the vendored IDLs (see scripts/dump-cpi-meta.mjs output).
#![allow(unexpected_cfgs)]
#![allow(dead_code)]

extern crate anchor_lang; // REQUIRED: declare_program! generated code references `super::anchor_lang`

use anchor_lang::declare_program;

declare_program!(kamino_lend);
declare_program!(jupiter_perps);
declare_program!(syrup_swap_pool);

/// Force full macro resolution of the three working CPI clients.
pub fn working_program_ids() -> [anchor_lang::prelude::Pubkey; 3] {
    [kamino_lend::ID, jupiter_perps::ID, syrup_swap_pool::ID]
}
