//! LiteSVM tests for ya-registry — lifecycle, authorization matrix, two-step governance rotation.
//! Run: cargo test -p ya-registry   (loads target/deploy/ya_registry.so)
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
    ya_registry::{accounts, instruction, AdapterStatus, Registry, ADAPTER_SEED, REGISTRY_SEED},
};

const SO: &[u8] = include_bytes!("../../../target/deploy/ya_registry.so");
const SYSTEM_PROGRAM: Pubkey = anchor_lang::solana_program::system_program::ID;

fn setup() -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    svm.add_program(ya_registry::ID, SO).unwrap();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    (svm, payer)
}

fn registry_pda() -> Pubkey {
    Pubkey::find_program_address(&[REGISTRY_SEED], &ya_registry::ID).0
}
fn entry_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[ADAPTER_SEED, program_id.as_ref()], &ya_registry::ID).0
}

/// Send a tx; returns Ok(()) on success, Err on failure. Always advances the blockhash.
fn send(svm: &mut LiteSVM, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> bool {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    let ok = svm.send_transaction(tx).is_ok();
    svm.expire_blockhash();
    ok
}

fn registry_state(svm: &LiteSVM) -> Registry {
    let acc = svm.get_account(&registry_pda()).unwrap();
    Registry::try_deserialize(&mut acc.data.as_slice()).unwrap()
}
fn entry_status(svm: &LiteSVM, program_id: &Pubkey) -> AdapterStatus {
    let acc = svm.get_account(&entry_pda(program_id)).unwrap();
    ya_registry::AdapterEntry::try_deserialize(&mut acc.data.as_slice())
        .unwrap()
        .status
}

// ── instruction builders ───────────────────────────────────
fn ix_init(payer: &Pubkey, gov: Pubkey, guardian: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        ya_registry::ID,
        &instruction::InitializeRegistry { governance: gov, guardian }.data(),
        accounts::InitializeRegistry {
            registry: registry_pda(),
            payer: *payer,
            system_program: SYSTEM_PROGRAM,
        }
        .to_account_metas(None),
    )
}
fn ix_propose(gov: &Pubkey, program_id: Pubkey, base_mint: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        ya_registry::ID,
        &instruction::ProposeAdapter {
            program_id,
            base_mint,
            name: "test-adapter".to_string(),
            version: 1,
            risk_tier: 2,
            remaining_accounts_hint: 8,
        }
        .data(),
        accounts::ProposeAdapter {
            registry: registry_pda(),
            adapter_entry: entry_pda(&program_id),
            governance: *gov,
            system_program: SYSTEM_PROGRAM,
        }
        .to_account_metas(None),
    )
}
fn ix_approve(gov: &Pubkey, program_id: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        ya_registry::ID,
        &instruction::ApproveAdapter {}.data(),
        accounts::GovernedEntry {
            registry: registry_pda(),
            adapter_entry: entry_pda(program_id),
            governance: *gov,
        }
        .to_account_metas(None),
    )
}
fn ix_resume(gov: &Pubkey, program_id: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        ya_registry::ID,
        &instruction::ResumeAdapter {}.data(),
        accounts::GovernedEntry {
            registry: registry_pda(),
            adapter_entry: entry_pda(program_id),
            governance: *gov,
        }
        .to_account_metas(None),
    )
}
fn ix_deprecate(gov: &Pubkey, program_id: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        ya_registry::ID,
        &instruction::DeprecateAdapter {}.data(),
        accounts::GovernedEntry {
            registry: registry_pda(),
            adapter_entry: entry_pda(program_id),
            governance: *gov,
        }
        .to_account_metas(None),
    )
}
fn ix_pause(authority: &Pubkey, program_id: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        ya_registry::ID,
        &instruction::PauseAdapter {}.data(),
        accounts::GuardedEntry {
            registry: registry_pda(),
            adapter_entry: entry_pda(program_id),
            authority: *authority,
        }
        .to_account_metas(None),
    )
}
fn ix_propose_gov(gov: &Pubkey, new_governance: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        ya_registry::ID,
        &instruction::ProposeGovernance { new_governance }.data(),
        accounts::Governed {
            registry: registry_pda(),
            governance: *gov,
        }
        .to_account_metas(None),
    )
}
fn ix_accept_gov(new_gov: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        ya_registry::ID,
        &instruction::AcceptGovernance {}.data(),
        accounts::AcceptGovernance {
            registry: registry_pda(),
            new_governance: *new_gov,
        }
        .to_account_metas(None),
    )
}

// ── tests ──────────────────────────────────────────────────
#[test]
fn full_lifecycle() {
    let (mut svm, payer) = setup();
    let gov = Keypair::new();
    let guardian = Keypair::new();
    let adapter = Keypair::new().pubkey();
    let base_mint = Keypair::new().pubkey();

    assert!(send(&mut svm, &[ix_init(&payer.pubkey(), gov.pubkey(), guardian.pubkey())], &payer, &[&payer]));
    assert_eq!(registry_state(&svm).governance, gov.pubkey());
    assert_eq!(registry_state(&svm).adapter_count, 0);

    // propose (governance signs + pays)
    svm.airdrop(&gov.pubkey(), 10_000_000_000).unwrap();
    assert!(send(&mut svm, &[ix_propose(&gov.pubkey(), adapter, base_mint)], &gov, &[&gov]));
    assert_eq!(entry_status(&svm, &adapter), AdapterStatus::Proposed);
    assert_eq!(registry_state(&svm).adapter_count, 1);

    // approve -> Active
    assert!(send(&mut svm, &[ix_approve(&gov.pubkey(), &adapter)], &gov, &[&gov]));
    assert_eq!(entry_status(&svm, &adapter), AdapterStatus::Active);

    // guardian pauses -> Paused
    svm.airdrop(&guardian.pubkey(), 10_000_000_000).unwrap();
    assert!(send(&mut svm, &[ix_pause(&guardian.pubkey(), &adapter)], &guardian, &[&guardian]));
    assert_eq!(entry_status(&svm, &adapter), AdapterStatus::Paused);

    // governance resumes -> Active
    assert!(send(&mut svm, &[ix_resume(&gov.pubkey(), &adapter)], &gov, &[&gov]));
    assert_eq!(entry_status(&svm, &adapter), AdapterStatus::Active);

    // governance deprecates -> Deprecated
    assert!(send(&mut svm, &[ix_deprecate(&gov.pubkey(), &adapter)], &gov, &[&gov]));
    assert_eq!(entry_status(&svm, &adapter), AdapterStatus::Deprecated);
}

#[test]
fn reinitialize_fails() {
    let (mut svm, payer) = setup();
    let gov = Keypair::new();
    let guardian = Keypair::new();
    assert!(send(&mut svm, &[ix_init(&payer.pubkey(), gov.pubkey(), guardian.pubkey())], &payer, &[&payer]));
    // second init must fail (init constraint)
    assert!(!send(&mut svm, &[ix_init(&payer.pubkey(), gov.pubkey(), guardian.pubkey())], &payer, &[&payer]));
}

#[test]
fn propose_requires_governance() {
    let (mut svm, payer) = setup();
    let gov = Keypair::new();
    let guardian = Keypair::new();
    let attacker = Keypair::new();
    let adapter = Keypair::new().pubkey();
    assert!(send(&mut svm, &[ix_init(&payer.pubkey(), gov.pubkey(), guardian.pubkey())], &payer, &[&payer]));
    svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();
    // attacker signs as "governance" -> has_one mismatch -> fail
    let ix = Instruction::new_with_bytes(
        ya_registry::ID,
        &instruction::ProposeAdapter {
            program_id: adapter,
            base_mint: Keypair::new().pubkey(),
            name: "x".into(),
            version: 1,
            risk_tier: 0,
            remaining_accounts_hint: 0,
        }
        .data(),
        accounts::ProposeAdapter {
            registry: registry_pda(),
            adapter_entry: entry_pda(&adapter),
            governance: attacker.pubkey(),
            system_program: SYSTEM_PROGRAM,
        }
        .to_account_metas(None),
    );
    assert!(!send(&mut svm, &[ix], &attacker, &[&attacker]));
}

#[test]
fn pause_by_random_fails_guardian_ok() {
    let (mut svm, payer) = setup();
    let gov = Keypair::new();
    let guardian = Keypair::new();
    let attacker = Keypair::new();
    let adapter = Keypair::new().pubkey();
    send(&mut svm, &[ix_init(&payer.pubkey(), gov.pubkey(), guardian.pubkey())], &payer, &[&payer]);
    svm.airdrop(&gov.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&guardian.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();
    send(&mut svm, &[ix_propose(&gov.pubkey(), adapter, Keypair::new().pubkey())], &gov, &[&gov]);
    send(&mut svm, &[ix_approve(&gov.pubkey(), &adapter)], &gov, &[&gov]);
    // attacker cannot pause
    assert!(!send(&mut svm, &[ix_pause(&attacker.pubkey(), &adapter)], &attacker, &[&attacker]));
    assert_eq!(entry_status(&svm, &adapter), AdapterStatus::Active);
    // guardian can
    assert!(send(&mut svm, &[ix_pause(&guardian.pubkey(), &adapter)], &guardian, &[&guardian]));
    assert_eq!(entry_status(&svm, &adapter), AdapterStatus::Paused);
}

#[test]
fn approve_requires_proposed_status() {
    let (mut svm, payer) = setup();
    let gov = Keypair::new();
    let guardian = Keypair::new();
    let adapter = Keypair::new().pubkey();
    send(&mut svm, &[ix_init(&payer.pubkey(), gov.pubkey(), guardian.pubkey())], &payer, &[&payer]);
    svm.airdrop(&gov.pubkey(), 10_000_000_000).unwrap();
    send(&mut svm, &[ix_propose(&gov.pubkey(), adapter, Keypair::new().pubkey())], &gov, &[&gov]);
    send(&mut svm, &[ix_approve(&gov.pubkey(), &adapter)], &gov, &[&gov]);
    // approving an already-Active adapter must fail (InvalidStatusTransition)
    assert!(!send(&mut svm, &[ix_approve(&gov.pubkey(), &adapter)], &gov, &[&gov]));
}

#[test]
fn two_step_governance_rotation() {
    let (mut svm, payer) = setup();
    let gov = Keypair::new();
    let guardian = Keypair::new();
    let new_gov = Keypair::new();
    let impostor = Keypair::new();
    send(&mut svm, &[ix_init(&payer.pubkey(), gov.pubkey(), guardian.pubkey())], &payer, &[&payer]);
    svm.airdrop(&gov.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&new_gov.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&impostor.pubkey(), 10_000_000_000).unwrap();

    // step 1: governance proposes new_gov
    assert!(send(&mut svm, &[ix_propose_gov(&gov.pubkey(), new_gov.pubkey())], &gov, &[&gov]));
    assert_eq!(registry_state(&svm).pending_governance, Some(new_gov.pubkey()));
    // governance hasn't changed yet
    assert_eq!(registry_state(&svm).governance, gov.pubkey());

    // impostor cannot accept
    assert!(!send(&mut svm, &[ix_accept_gov(&impostor.pubkey())], &impostor, &[&impostor]));

    // new_gov accepts -> becomes governance
    assert!(send(&mut svm, &[ix_accept_gov(&new_gov.pubkey())], &new_gov, &[&new_gov]));
    assert_eq!(registry_state(&svm).governance, new_gov.pubkey());
    assert_eq!(registry_state(&svm).pending_governance, None);

    // old governance can no longer act
    let adapter = Keypair::new().pubkey();
    assert!(!send(&mut svm, &[ix_propose(&gov.pubkey(), adapter, Keypair::new().pubkey())], &gov, &[&gov]));
}
