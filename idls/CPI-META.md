# CPI metadata (auto-dumped from vendored IDLs)

Manual-CPI adapters (marginfi, drift) build `Instruction{ data: discriminator ++ borsh(args), accounts }`.

## kamino_lend  (KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD)

### ix `deposit_reserve_liquidity`
- discriminator: [169,201,30,126,6,205,102,68]
- args: liquidity_amount: u64
- accounts (12):
    0. owner (signer)
    1. reserve (mut)
    2. lending_market
    3. lending_market_authority
    4. reserve_liquidity_mint
    5. reserve_liquidity_supply (mut)
    6. reserve_collateral_mint (mut)
    7. user_source_liquidity (mut)
    8. user_destination_collateral (mut)
    9. collateral_token_program
    10. liquidity_token_program
    11. instruction_sysvar_account

### ix `redeem_reserve_collateral`
- discriminator: [234,117,181,125,185,142,220,29]
- args: collateral_amount: u64
- accounts (12):
    0. owner (signer)
    1. lending_market
    2. reserve (mut)
    3. lending_market_authority
    4. reserve_liquidity_mint
    5. reserve_collateral_mint (mut)
    6. reserve_liquidity_supply (mut)
    7. user_source_collateral (mut)
    8. user_destination_liquidity (mut)
    9. collateral_token_program
    10. liquidity_token_program
    11. instruction_sysvar_account

### ix `refresh_reserve`
- discriminator: [2,218,138,235,79,201,25,102]
- args: (none)
- accounts (6):
    0. reserve (mut)
    1. lending_market
    2. pyth_oracle (optional)
    3. switchboard_price_oracle (optional)
    4. switchboard_twap_oracle (optional)
    5. scope_prices (optional)

### account `Reserve`  discriminator: [43,242,204,202,26,247,59,127]
- fields:
    - version: u64
    - last_update: LastUpdate
    - lending_market: pubkey
    - farm_collateral: pubkey
    - farm_debt: pubkey
    - liquidity: ReserveLiquidity
    - reserve_liquidity_padding: [u64; 150]
    - collateral: ReserveCollateral
    - reserve_collateral_padding: [u64; 150]
    - config: ReserveConfig
    - config_padding: [u64; 113]
    - borrowed_amount_outside_elevation_group: u64
    - borrowed_amounts_against_this_reserve_in_elevation_groups: [u64; 32]
    - withdraw_queue: WithdrawQueue
    - padding: [u64; 204]

## marginfi  (MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA)

### ix `marginfi_account_initialize`
- discriminator: [43,78,61,255,148,52,249,154]
- args: (none)
- accounts (5):
    0. marginfi_group
    1. marginfi_account (mut,signer)
    2. authority (signer)
    3. fee_payer (mut,signer)
    4. system_program

### ix `lending_account_deposit`
- discriminator: [171,94,235,103,82,64,212,140]
- args: amount: u64, deposit_up_to_limit: Option<bool>
- accounts (7):
    0. group
    1. marginfi_account (mut)
    2. authority (signer)
    3. bank (mut)
    4. signer_token_account (mut)
    5. liquidity_vault (mut)
    6. token_program

### ix `lending_account_withdraw`
- discriminator: [36,72,74,19,210,210,192,192]
- args: amount: u64, withdraw_all: Option<bool>
- accounts (8):
    0. group
    1. marginfi_account (mut)
    2. authority (signer)
    3. bank (mut)
    4. destination_token_account (mut)
    5. bank_liquidity_vault_authority (pda)
    6. liquidity_vault (mut)
    7. token_program

### account `MarginfiAccount`  discriminator: [67,178,130,109,126,114,28,42]
- fields:
    - group: pubkey
    - authority: pubkey
    - lending_account: LendingAccount
    - account_flags: u64
    - emissions_destination_account: pubkey
    - health_cache: HealthCache
    - migrated_from: pubkey
    - migrated_to: pubkey
    - last_update: u64
    - account_index: u16
    - third_party_index: u16
    - bump: u8
    - _pad0: [u8; 3]
    - liquidation_record: pubkey
    - _padding0: [u64; 7]

### account `Bank`  discriminator: [142,49,166,242,50,66,97,188]
- fields:
    - mint: pubkey
    - mint_decimals: u8
    - group: pubkey
    - _pad0: [u8; 7]
    - asset_share_value: WrappedI80F48
    - liability_share_value: WrappedI80F48
    - liquidity_vault: pubkey
    - liquidity_vault_bump: u8
    - liquidity_vault_authority_bump: u8
    - insurance_vault: pubkey
    - insurance_vault_bump: u8
    - insurance_vault_authority_bump: u8
    - _pad1: [u8; 4]
    - collected_insurance_fees_outstanding: WrappedI80F48
    - fee_vault: pubkey
    - fee_vault_bump: u8
    - fee_vault_authority_bump: u8
    - _pad2: [u8; 6]
    - collected_group_fees_outstanding: WrappedI80F48
    - total_liability_shares: WrappedI80F48
    - total_asset_shares: WrappedI80F48
    - last_update: i64
    - config: BankConfig
    - flags: u64
    - emissions_rate: u64
    - emissions_remaining: WrappedI80F48
    - emissions_mint: pubkey
    - collected_program_fees_outstanding: WrappedI80F48
    - emode: EmodeSettings
    - fees_destination_account: pubkey
    - cache: BankCache
    - lending_position_count: i32
    - borrowing_position_count: i32
    - _padding_0: [u8; 16]
    - integration_acc_1: pubkey
    - integration_acc_2: pubkey
    - integration_acc_3: pubkey
    - rate_limiter: BankRateLimiter
    - _pad_0: [u8; 16]
    - _padding_1: [[u64; 2]; 7]

## jupiter_perps  (PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu)

### ix `add_liquidity2`
- discriminator: [228,162,78,28,70,219,116,115]
- args: params: AddLiquidity2Params
- accounts (14):
    0. owner (signer)
    1. funding_account (mut)
    2. lp_token_account (mut)
    3. transfer_authority
    4. perpetuals
    5. pool (mut)
    6. custody (mut)
    7. custody_doves_price_account
    8. custody_pythnet_price_account
    9. custody_token_account (mut)
    10. lp_token_mint (mut)
    11. token_program
    12. event_authority
    13. program

### ix `remove_liquidity2`
- discriminator: [230,215,82,127,241,101,227,146]
- args: params: RemoveLiquidity2Params
- accounts (14):
    0. owner (signer)
    1. receiving_account (mut)
    2. lp_token_account (mut)
    3. transfer_authority
    4. perpetuals
    5. pool (mut)
    6. custody (mut)
    7. custody_doves_price_account
    8. custody_pythnet_price_account
    9. custody_token_account (mut)
    10. lp_token_mint (mut)
    11. token_program
    12. event_authority
    13. program

### account `Pool`  discriminator: [241,154,109,4,17,177,109,188]
- fields:
    - name: string
    - custodies: Vec<pubkey>
    - aum_usd: u128
    - limit: Limit
    - fees: Fees
    - pool_apr: PoolApr
    - max_request_execution_sec: i64
    - bump: u8
    - lp_token_bump: u8
    - inception_time: i64
    - parameter_update_oracle: Secp256k1Pubkey
    - aum_usd_updated_at: i64

### account `Custody`  discriminator: [1,184,48,81,93,131,63,145]
- fields:
    - pool: pubkey
    - mint: pubkey
    - token_account: pubkey
    - decimals: u8
    - is_stable: bool
    - oracle: OracleParams
    - pricing: PricingParams
    - permissions: Permissions
    - target_ratio_bps: u64
    - assets: Assets
    - funding_rate_state: FundingRateState
    - bump: u8
    - token_account_bump: u8
    - increase_position_bps: u64
    - decrease_position_bps: u64
    - max_position_size_usd: u64
    - doves_oracle: pubkey
    - jump_rate_state: JumpRateState
    - doves_ag_oracle: pubkey
    - price_impact_buffer: PriceImpactBuffer
    - borrow_lend_parameters: BorrowLendParams
    - borrows_funding_rate_state: FundingRateState
    - debt: u128
    - borrow_lend_interests_accured: u128
    - borrow_limit_in_token_amount: u64
    - min_interest_fee_bps: u64
    - min_interest_fee_grace_period_seconds: u64

## drift  (dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH)

### ix `initialize_user_stats`
- discriminator: [254,243,72,98,251,130,168,213]
- args: (none)
- accounts (6):
    0. user_stats (mut)
    1. state (mut)
    2. authority
    3. payer (mut,signer)
    4. rent
    5. system_program

### ix `initialize_insurance_fund_stake`
- discriminator: [187,179,243,70,248,90,92,147]
- args: market_index: u16
- accounts (8):
    0. spot_market
    1. insurance_fund_stake (mut)
    2. user_stats (mut)
    3. state
    4. authority (signer)
    5. payer (mut,signer)
    6. rent
    7. system_program

### ix `add_insurance_fund_stake`
- discriminator: [251,144,115,11,222,47,62,236]
- args: market_index: u16, amount: u64
- accounts (10):
    0. state
    1. spot_market (mut)
    2. insurance_fund_stake (mut)
    3. user_stats (mut)
    4. authority (signer)
    5. spot_market_vault (mut)
    6. insurance_fund_vault (mut)
    7. drift_signer
    8. user_token_account (mut)
    9. token_program

### ix `request_remove_insurance_fund_stake`
- discriminator: [142,70,204,92,73,106,180,52]
- args: market_index: u16, amount: u64
- accounts (5):
    0. spot_market (mut)
    1. insurance_fund_stake (mut)
    2. user_stats (mut)
    3. authority (signer)
    4. insurance_fund_vault (mut)

### ix `remove_insurance_fund_stake`
- discriminator: [128,166,142,9,254,187,143,174]
- args: market_index: u16
- accounts (9):
    0. state
    1. spot_market (mut)
    2. insurance_fund_stake (mut)
    3. user_stats (mut)
    4. authority (signer)
    5. insurance_fund_vault (mut)
    6. drift_signer
    7. user_token_account (mut)
    8. token_program

### ix `cancel_request_remove_insurance_fund_stake`
- discriminator: [97,235,78,62,212,42,241,127]
- args: market_index: u16
- accounts (5):
    0. spot_market (mut)
    1. insurance_fund_stake (mut)
    2. user_stats (mut)
    3. authority (signer)
    4. insurance_fund_vault (mut)

### account `InsuranceFundStake`  discriminator: [110,202,14,42,95,73,90,95]
- fields:
    - authority: pubkey
    - if_shares: u128
    - last_withdraw_request_shares: u128
    - if_base: u128
    - last_valid_ts: i64
    - last_withdraw_request_value: u64
    - last_withdraw_request_ts: i64
    - cost_basis: i64
    - market_index: u16
    - padding: [u8; 14]

### account `SpotMarket`  discriminator: [100,177,8,107,168,65,65,39]
- fields:
    - pubkey: pubkey
    - oracle: pubkey
    - mint: pubkey
    - vault: pubkey
    - name: [u8; 32]
    - historical_oracle_data: HistoricalOracleData
    - historical_index_data: HistoricalIndexData
    - revenue_pool: PoolBalance
    - spot_fee_pool: PoolBalance
    - insurance_fund: InsuranceFund
    - total_spot_fee: u128
    - deposit_balance: u128
    - borrow_balance: u128
    - cumulative_deposit_interest: u128
    - cumulative_borrow_interest: u128
    - total_social_loss: u128
    - total_quote_social_loss: u128
    - withdraw_guard_threshold: u64
    - max_token_deposits: u64
    - deposit_token_twap: u64
    - borrow_token_twap: u64
    - utilization_twap: u64
    - last_interest_ts: u64
    - last_twap_ts: u64
    - expiry_ts: i64
    - order_step_size: u64
    - order_tick_size: u64
    - min_order_size: u64
    - max_position_size: u64
    - next_fill_record_id: u64
    - next_deposit_record_id: u64
    - initial_asset_weight: u32
    - maintenance_asset_weight: u32
    - initial_liability_weight: u32
    - maintenance_liability_weight: u32
    - imf_factor: u32
    - liquidator_fee: u32
    - if_liquidation_fee: u32
    - optimal_utilization: u32
    - optimal_borrow_rate: u32
    - max_borrow_rate: u32
    - decimals: u32
    - market_index: u16
    - orders_enabled: bool
    - oracle_source: OracleSource
    - status: MarketStatus
    - asset_tier: AssetTier
    - paused_operations: u8
    - if_paused_operations: u8
    - fee_adjustment: i16
    - max_token_borrows_fraction: u16
    - flash_loan_amount: u64
    - flash_loan_initial_token_amount: u64
    - total_swap_fee: u64
    - scale_initial_asset_weight_start: u64
    - min_borrow_rate: u8
    - fuel_boost_deposits: u8
    - fuel_boost_borrows: u8
    - fuel_boost_taker: u8
    - fuel_boost_maker: u8
    - fuel_boost_insurance: u8
    - token_program_flag: u8
    - pool_id: u8
    - padding: [u8; 40]

### account `UserStats`  discriminator: [176,223,136,27,122,79,32,227]
- fields:
    - authority: pubkey
    - referrer: pubkey
    - fees: UserFees
    - next_epoch_ts: i64
    - maker_volume30d: u64
    - taker_volume30d: u64
    - filler_volume30d: u64
    - last_maker_volume30d_ts: i64
    - last_taker_volume30d_ts: i64
    - last_filler_volume30d_ts: i64
    - if_staked_quote_asset_amount: u64
    - number_of_sub_accounts: u16
    - number_of_sub_accounts_created: u16
    - referrer_status: u8
    - disable_update_perp_bid_ask_twap: u8
    - paused_operations: u8
    - fuel_overflow_status: u8
    - fuel_insurance: u32
    - fuel_deposits: u32
    - fuel_borrows: u32
    - fuel_positions: u32
    - fuel_taker: u32
    - fuel_maker: u32
    - if_staked_gov_token_amount: u64
    - last_fuel_if_bonus_update_ts: u32
    - padding: [u8; 12]

## syrup_swap_pool  (whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc)

### ix `swap`
- discriminator: [248,198,158,145,225,117,135,200]
- args: amount: u64, other_amount_threshold: u64, sqrt_price_limit: u128, amount_specified_is_input: bool, a_to_b: bool
- accounts (11):
    0. token_program
    1. token_authority (signer)
    2. whirlpool (mut)
    3. token_owner_account_a (mut)
    4. token_vault_a (mut)
    5. token_owner_account_b (mut)
    6. token_vault_b (mut)
    7. tick_array_0 (mut)
    8. tick_array_1 (mut)
    9. tick_array_2 (mut)
    10. oracle (pda)

### ix `swap_v2`
- discriminator: [43,4,237,11,26,201,30,98]
- args: amount: u64, other_amount_threshold: u64, sqrt_price_limit: u128, amount_specified_is_input: bool, a_to_b: bool, remaining_accounts_info: Option<RemainingAccountsInfo>
- accounts (15):
    0. token_program_a
    1. token_program_b
    2. memo_program
    3. token_authority (signer)
    4. whirlpool (mut)
    5. token_mint_a
    6. token_mint_b
    7. token_owner_account_a (mut)
    8. token_vault_a (mut)
    9. token_owner_account_b (mut)
    10. token_vault_b (mut)
    11. tick_array_0 (mut)
    12. tick_array_1 (mut)
    13. tick_array_2 (mut)
    14. oracle (mut,pda)

### account `Whirlpool`  discriminator: [63,149,209,12,225,128,99,9]
- fields:
    - whirlpools_config: pubkey
    - whirlpool_bump: [u8; 1]
    - tick_spacing: u16
    - fee_tier_index_seed: [u8; 2]
    - fee_rate: u16
    - protocol_fee_rate: u16
    - liquidity: u128
    - sqrt_price: u128
    - tick_current_index: i32
    - protocol_fee_owed_a: u64
    - protocol_fee_owed_b: u64
    - token_mint_a: pubkey
    - token_vault_a: pubkey
    - fee_growth_global_a: u128
    - token_mint_b: pubkey
    - token_vault_b: pubkey
    - fee_growth_global_b: u128
    - reward_last_updated_timestamp: u64
    - reward_infos: [WhirlpoolRewardInfo; 3]
