//! FSM and quorum math verification tests for governance proposal state transitions.

#[cfg(test)]
mod tests {
    use crate::governance;
    use crate::types::{Proposal, ProposalState};
    use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, vec, Address, BytesN, Env};

    #[soroban_sdk::contract]
    pub struct TestContract;

    #[soroban_sdk::contractimpl]
    impl TestContract {}

    fn setup_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    /// Test: Proposal starts in Pending state
    #[test]
    fn test_proposal_initial_state_pending() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let signers = vec![&env, s1.clone()];
        let weights = vec![&env, 100];
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 5000); // 50% threshold

            let p = Proposal {
                id: 1,
                action_hash: BytesN::from_array(&env, &[0u8; 32]),
                proposer: s1.clone(),
                approved_by: vec![&env],
                state: ProposalState::Pending,
            };

            let pid = governance::propose(&env, p);
            assert_eq!(pid, 1);
        });
    }

    /// Test: Pending → Approved transition on threshold met
    #[test]
    fn test_state_transition_pending_to_approved() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);
        let signers = vec![&env, s1.clone(), s2.clone()];
        let weights = vec![&env, 100, 200];
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 5000); // 50.00% threshold (needs 150 weight out of 300)

            let p = Proposal {
                id: 1,
                action_hash: BytesN::from_array(&env, &[0u8; 32]),
                proposer: s1.clone(),
                approved_by: vec![&env],
                state: ProposalState::Pending,
            };
            governance::propose(&env, p);

            // s1 approves: weight 100/300 = 33.33% < 50.00% -> should remain Pending
            governance::approve(&env, &s1, 1);
            let props = env.storage().instance().get::<_, soroban_sdk::Map<u64, (Proposal, u32)>>(&soroban_sdk::symbol_short!("PROPS")).unwrap();
            let (prop_s1, _) = props.get(1).unwrap();
            assert_eq!(prop_s1.state, ProposalState::Pending);

            // s2 approves: total weight becomes 300/300 = 100% >= 50% -> should transition to Approved
            governance::approve(&env, &s2, 1);
            let props = env.storage().instance().get::<_, soroban_sdk::Map<u64, (Proposal, u32)>>(&soroban_sdk::symbol_short!("PROPS")).unwrap();
            let (prop_s2, _) = props.get(1).unwrap();
            assert_eq!(prop_s2.state, ProposalState::Approved);
        });
    }

    /// Test: Approved → Executed transition on timelock expiry
    #[test]
    fn test_state_transition_approved_to_executed() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let signers = vec![&env, s1.clone()];
        let weights = vec![&env, 100];
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 10000); // 100.00% threshold

            let p = Proposal {
                id: 1,
                action_hash: BytesN::from_array(&env, &[0u8; 32]),
                proposer: s1.clone(),
                approved_by: vec![&env],
                state: ProposalState::Pending,
            };
            governance::propose(&env, p);

            governance::approve(&env, &s1, 1);

            // Advance ledger to trigger timelock expiry (unlock = start + 720)
            env.ledger().set_sequence_number(800);
            let executed_prop = governance::execute(&env, 1);
            assert_eq!(executed_prop.state, ProposalState::Executed);
        });
    }

    /// Test: Rejecting approvals on Approved proposals (invalid transition)
    #[test]
    #[should_panic]
    fn test_reject_approval_on_approved_proposal() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);
        let signers = vec![&env, s1.clone(), s2.clone()];
        let weights = vec![&env, 100, 100];
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 5000); // 50.00%

            let p = Proposal {
                id: 1,
                action_hash: BytesN::from_array(&env, &[0u8; 32]),
                proposer: s1.clone(),
                approved_by: vec![&env],
                state: ProposalState::Pending,
            };
            governance::propose(&env, p);

            governance::approve(&env, &s1, 1); // transitions to Approved since weight 100/200 = 50.00% >= 50.00%
            governance::approve(&env, &s2, 1); // should panic with InvalidStateTransition
        });
    }

    /// Test: Rejecting execution of Pending proposals
    #[test]
    #[should_panic]
    fn test_reject_execution_of_pending_proposal() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let signers = vec![&env, s1.clone()];
        let weights = vec![&env, 100];
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 10000);

            let p = Proposal {
                id: 1,
                action_hash: BytesN::from_array(&env, &[0u8; 32]),
                proposer: s1.clone(),
                approved_by: vec![&env],
                state: ProposalState::Pending,
            };
            governance::propose(&env, p);

            // execute without approving
            governance::execute(&env, 1);
        });
    }

    /// Test: Rejecting double-execution of Executed proposals
    #[test]
    #[should_panic]
    fn test_reject_double_execution() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let signers = vec![&env, s1.clone()];
        let weights = vec![&env, 100];
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 10000);

            let p = Proposal {
                id: 1,
                action_hash: BytesN::from_array(&env, &[0u8; 32]),
                proposer: s1.clone(),
                approved_by: vec![&env],
                state: ProposalState::Pending,
            };
            governance::propose(&env, p);
            governance::approve(&env, &s1, 1);

            env.ledger().set_sequence_number(800);
            governance::execute(&env, 1);
            governance::execute(&env, 1); // should panic
        });
    }

    /// Test: Rejecting approval of Executed proposals
    #[test]
    #[should_panic]
    fn test_reject_approval_of_executed_proposal() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);
        let signers = vec![&env, s1.clone(), s2.clone()];
        let weights = vec![&env, 100, 100];
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 5000);

            let p = Proposal {
                id: 1,
                action_hash: BytesN::from_array(&env, &[0u8; 32]),
                proposer: s1.clone(),
                approved_by: vec![&env],
                state: ProposalState::Pending,
            };
            governance::propose(&env, p);
            governance::approve(&env, &s1, 1);

            env.ledger().set_sequence_number(800);
            governance::execute(&env, 1);

            governance::approve(&env, &s2, 1); // should panic
        });
    }

    /// Test: Duplicate approval detection
    #[test]
    #[should_panic]
    fn test_duplicate_approval_detection() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let signers = vec![&env, s1.clone()];
        let weights = vec![&env, 100];
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 10000);

            let p = Proposal {
                id: 1,
                action_hash: BytesN::from_array(&env, &[0u8; 32]),
                proposer: s1.clone(),
                approved_by: vec![&env],
                state: ProposalState::Pending,
            };
            governance::propose(&env, p);

            governance::approve(&env, &s1, 1);
            governance::approve(&env, &s1, 1); // duplicate should panic
        });
    }

    /// Test: Input validations on init
    #[test]
    #[should_panic]
    fn test_init_mismatched_signers_and_weights() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let signers = vec![&env, s1];
        let weights = vec![&env]; // empty
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 5000);
        });
    }

    #[test]
    #[should_panic]
    fn test_init_invalid_threshold() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let signers = vec![&env, s1];
        let weights = vec![&env, 100];
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 10001); // threshold > 10,000 basis points
        });
    }

    #[test]
    #[should_panic]
    fn test_init_zero_weight() {
        let env = setup_env();
        let s1 = Address::generate(&env);
        let signers = vec![&env, s1];
        let weights = vec![&env, 0]; // weight of 0
        let contract_id = env.register_contract(None, TestContract);
        env.as_contract(&contract_id, || {
            governance::init(&env, signers, weights, 5000);
        });
    }

    /// Test: Strict quorum math exact boundary condition
    #[test]
    fn test_quorum_exact_boundary() {
        // Test case 1: 66.67% threshold (needs > 2 approvals of 3 equal-weight signers)
        {
            let env = setup_env();
            let s1 = Address::generate(&env);
            let s2 = Address::generate(&env);
            let s3 = Address::generate(&env);
            let signers = vec![&env, s1.clone(), s2.clone(), s3.clone()];
            let weights = vec![&env, 100, 100, 100]; // total weight = 300

            let contract_id = env.register_contract(None, TestContract);
            env.as_contract(&contract_id, || {
                governance::init(&env, signers, weights, 6667);
                let p1 = Proposal {
                    id: 1,
                    action_hash: BytesN::from_array(&env, &[0u8; 32]),
                    proposer: s1.clone(),
                    approved_by: vec![&env],
                    state: ProposalState::Pending,
                };
                governance::propose(&env, p1);
                governance::approve(&env, &s1, 1);
                governance::approve(&env, &s2, 1);

                // 200 * 10000 = 2,000,000. 6667 * 300 = 2,000,100.
                // 2,000,000 < 2,000,100 -> not met.
                let props = env.storage().instance().get::<_, soroban_sdk::Map<u64, (Proposal, u32)>>(&soroban_sdk::symbol_short!("PROPS")).unwrap();
                let (prop1, _) = props.get(1).unwrap();
                assert_eq!(prop1.state, ProposalState::Pending);
            });
        }

        // Test case 2: 66.66% threshold (needs >= 2 approvals of 3 equal-weight signers)
        {
            let env2 = setup_env();
            let s1 = Address::generate(&env2);
            let s2 = Address::generate(&env2);
            let s3 = Address::generate(&env2);
            let signers = vec![&env2, s1.clone(), s2.clone(), s3.clone()];
            let weights = vec![&env2, 100, 100, 100];

            let contract_id2 = env2.register_contract(None, TestContract);
            env2.as_contract(&contract_id2, || {
                governance::init(&env2, signers, weights, 6666);
                let p2 = Proposal {
                    id: 1,
                    action_hash: BytesN::from_array(&env2, &[0u8; 32]),
                    proposer: s1.clone(),
                    approved_by: vec![&env2],
                    state: ProposalState::Pending,
                };
                governance::propose(&env2, p2);
                governance::approve(&env2, &s1, 1);
                governance::approve(&env2, &s2, 1);

                // 200 * 10000 = 2,000,000. 6666 * 300 = 1,999,800.
                // 2,000,000 >= 1,999,800 -> met!
                let props2 = env2.storage().instance().get::<_, soroban_sdk::Map<u64, (Proposal, u32)>>(&soroban_sdk::symbol_short!("PROPS")).unwrap();
                let (prop2, _) = props2.get(1).unwrap();
                assert_eq!(prop2.state, ProposalState::Approved);
            });
        }
    }
}
