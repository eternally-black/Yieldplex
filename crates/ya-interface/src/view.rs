//! `current_value` as a Solana-native view (SPEC §4.2 / §5.1).
//!
//! An adapter's `current_value` computes the position's redeemable base-asset value, caches it
//! on `Position`, and exposes it via return data using [`report_value`]. Callers read it off
//! `simulateTransaction` returnData, or on-chain after a CPI via [`read_returned_value`]. The
//! dispatcher's `route_current_value` reads the adapter's value this way and re-`report_value`s it,
//! so the dispatcher is itself view-callable.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::{get_return_data, set_return_data};

/// Expose a base-asset `u64` value as return data (little-endian, 8 bytes).
pub fn report_value(value: u64) {
    set_return_data(&value.to_le_bytes());
}

/// Read a `u64` value previously returned by `expected_program` (e.g. an adapter, read by the
/// dispatcher after CPI). Returns `None` if no data, wrong program, or malformed length.
pub fn read_returned_value(expected_program: &Pubkey) -> Option<u64> {
    let (program, data) = get_return_data()?;
    if &program != expected_program || data.len() < 8 {
        return None;
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[..8]);
    Some(u64::from_le_bytes(buf))
}
