//! LiteSVM e2e for the dispatcher router: registry → dispatcher → mock adapter.
//! Validates the full route path, the return-data view, and registry gating (Active / base-mint).
use {
    anchor_lang::{
        prelude::Pubkey, solana_program::instruction::Instruction, AccountDeserialize,
        InstructionData, ToAccountMetas,
    },
    litesvm::LiteSVM,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    ya_interface::constants::seeds,
    ya_registry::{ADAPTER_SEED, REGISTRY_SEED},
};

const REGISTRY_SO: &[u8] = include_bytes!("../../../target/deploy/ya_registry.so");
const DISPATCHER_SO: &[u8] = include_bytes!("../../../target/deploy/ya_dispatcher.so");
const MOCK_SO: &[u8] = include_bytes!("../../../target/deploy/ya_mock_adapter.so");
const SYSTEM_PROGRAM: Pubkey = anchor_lang::solana_program::system_program::ID;

struct Env {
    svm: LiteSVM,
    gov: Keypair,
    owner: Keypair,
    base_mint: Pubkey,
}

fn registry_pda() -> Pubkey {
    Pubkey::find_program_address(&[REGISTRY_SEED], &ya_registry::ID).0
}
fn entry_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[ADAPTER_SEED, program_id.as_ref()], &ya_registry::ID).0
}
fn position_pda(owner: &Pubkey, base_mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[seeds::POSITION, owner.as_ref(), base_mint.as_ref()],
        &ya_mock_adapter::ID,
    )
    .0
}
fn vault_authority_pda(position: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[seeds::VAULT_AUTHORITY, position.as_ref()], &ya_mock_adapter::ID).0
}

fn send(svm: &mut LiteSVM, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> bool {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    let ok = svm.send_transaction(tx).is_ok();
    svm.expire_blockhash();
    ok
}

/// Stand up registry + a registered (Active) mock adapter + an initialized position.
fn bootstrap() -> Env {
    let mut svm = LiteSVM::new();
    svm.add_program(ya_registry::ID, REGISTRY_SO).unwrap();
    svm.add_program(ya_dispatcher::ID, DISPATCHER_SO).unwrap();
    svm.add_program(ya_mock_adapter::ID, MOCK_SO).unwrap();

    let payer = Keypair::new();
    let gov = Keypair::new();
    let owner = Keypair::new();
    let base_mint = Pubkey::new_unique();
    for kp in [&payer, &gov, &owner] {
        svm.airdrop(&kp.pubkey(), 100_000_000_000).unwrap();
    }

    // init registry
    let init = Instruction::new_with_bytes(
        ya_registry::ID,
        &ya_registry::instruction::InitializeRegistry { governance: gov.pubkey(), guardian: gov.pubkey() }.data(),
        ya_registry::accounts::InitializeRegistry { registry: registry_pda(), payer: payer.pubkey(), system_program: SYSTEM_PROGRAM }.to_account_metas(None),
    );
    assert!(send(&mut svm, &[init], &payer, &[&payer]));

    // propose + approve the mock adapter
    let propose = Instruction::new_with_bytes(
        ya_registry::ID,
        &ya_registry::instruction::ProposeAdapter {
            program_id: ya_mock_adapter::ID,
            base_mint,
            name: "mock".into(),
            version: 1,
            risk_tier: 0,
            remaining_accounts_hint: 9,
        }
        .data(),
        ya_registry::accounts::ProposeAdapter { registry: registry_pda(), adapter_entry: entry_pda(&ya_mock_adapter::ID), governance: gov.pubkey(), system_program: SYSTEM_PROGRAM }.to_account_metas(None),
    );
    assert!(send(&mut svm, &[propose], &gov, &[&gov]));
    let approve = Instruction::new_with_bytes(
        ya_registry::ID,
        &ya_registry::instruction::ApproveAdapter {}.data(),
        ya_registry::accounts::GovernedEntry { registry: registry_pda(), adapter_entry: entry_pda(&ya_mock_adapter::ID), governance: gov.pubkey() }.to_account_metas(None),
    );
    assert!(send(&mut svm, &[approve], &gov, &[&gov]));

    // initialize_position directly on the mock adapter
    let position = position_pda(&owner.pubkey(), &base_mint);
    let init_pos = Instruction::new_with_bytes(
        ya_mock_adapter::ID,
        &ya_mock_adapter::instruction::InitializePosition {}.data(),
        ya_mock_adapter::accounts::InitializePosition {
            position,
            vault_authority: vault_authority_pda(&position),
            base_mint,
            owner: owner.pubkey(),
            system_program: SYSTEM_PROGRAM,
        }
        .to_account_metas(None),
    );
    assert!(send(&mut svm, &[init_pos], &owner, &[&owner]));

    Env { svm, gov, owner, base_mint }
}

/// Build a dispatcher Route instruction with the given action data + base_mint (for mismatch tests).
fn route_ix(env: &Env, base_mint: Pubkey, data: Vec<u8>) -> Instruction {
    let position = position_pda(&env.owner.pubkey(), &env.base_mint);
    Instruction::new_with_bytes(
        ya_dispatcher::ID,
        &data,
        ya_dispatcher::accounts::Route {
            position,
            vault_authority: vault_authority_pda(&position),
            base_mint,
            vault_token_account: Pubkey::new_unique(),
            owner: env.owner.pubkey(),
            owner_token_account: Pubkey::new_unique(),
            registry_entry: entry_pda(&ya_mock_adapter::ID),
            token_program: Pubkey::new_unique(),
            system_program: SYSTEM_PROGRAM,
            adapter_program: ya_mock_adapter::ID,
        }
        .to_account_metas(None),
    )
}

fn position_shares(svm: &LiteSVM, owner: &Pubkey, base_mint: &Pubkey) -> u64 {
    let acc = svm.get_account(&position_pda(owner, base_mint)).unwrap();
    ya_mock_adapter::Position::try_deserialize(&mut acc.data.as_slice())
        .unwrap()
        .shares
}

#[test]
fn route_deposit_and_current_value() {
    let mut env = bootstrap();
    let bm = env.base_mint;

    // route_deposit(25, 0) through the dispatcher -> mock
    let dep = route_ix(&env, bm, ya_dispatcher::instruction::RouteDeposit { amount: 25_000_000, min_position_out: 0 }.data());
    assert!(send(&mut env.svm, &[dep], &env.owner, &[&env.owner]));
    assert_eq!(position_shares(&env.svm, &env.owner.pubkey(), &bm), 25_000_000);

    // route_current_value -> dispatcher reads adapter return data + re-returns it
    let cv = route_ix(&env, bm, ya_dispatcher::instruction::RouteCurrentValue {}.data());
    let blockhash = env.svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[cv], Some(&env.owner.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&env.owner]).unwrap();
    let meta = env.svm.send_transaction(tx).expect("route_current_value ok");
    let data = meta.return_data.data;
    assert_eq!(data.len(), 8, "value returned as u64 LE");
    let value = u64::from_le_bytes(data[..8].try_into().unwrap());
    assert_eq!(value, 25_000_000, "dispatcher re-returns the adapter's value");
}

#[test]
fn paused_adapter_blocks_routing() {
    let mut env = bootstrap();
    let bm = env.base_mint;
    // guardian/governance pauses the mock
    let pause = Instruction::new_with_bytes(
        ya_registry::ID,
        &ya_registry::instruction::PauseAdapter {}.data(),
        ya_registry::accounts::GuardedEntry { registry: registry_pda(), adapter_entry: entry_pda(&ya_mock_adapter::ID), authority: env.gov.pubkey() }.to_account_metas(None),
    );
    assert!(send(&mut env.svm, &[pause], &env.gov, &[&env.gov]));
    // routing now fails (AdapterNotActive)
    let dep = route_ix(&env, bm, ya_dispatcher::instruction::RouteDeposit { amount: 1, min_position_out: 0 }.data());
    assert!(!send(&mut env.svm, &[dep], &env.owner, &[&env.owner]));
}

#[test]
fn base_mint_mismatch_blocks_routing() {
    let mut env = bootstrap();
    let wrong_mint = Pubkey::new_unique();
    // dispatcher gate compares passed base_mint to the registry entry's base_mint
    let dep = route_ix(&env, wrong_mint, ya_dispatcher::instruction::RouteDeposit { amount: 1, min_position_out: 0 }.data());
    assert!(!send(&mut env.svm, &[dep], &env.owner, &[&env.owner]));
}
