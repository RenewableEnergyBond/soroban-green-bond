#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

// ---------------------------------------------------------------------------
// Initialisation tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_stores_bond_info() {
    // We use a mock whitelist address for unit tests
    let env = Env::default();
    env.mock_all_auths();
    let mock_whitelist = Address::generate(&env);

    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);

    bond.initialize(
        &issuer,
        &1_000_000_i128,
        &1_900_000_000_u64,
        &500_u32,
        &String::from_str(&env, "FRRBD00001"),
        &mock_whitelist,
    );

    let info = bond.get_bond_info();
    assert_eq!(info.issuer, issuer);
    assert_eq!(info.total_supply, 1_000_000);
    assert_eq!(info.minted_supply, 0);
    assert_eq!(info.maturity_timestamp, 1_900_000_000);
    assert_eq!(info.coupon_rate_bps, 500);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_double_initialize_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let mock_whitelist = Address::generate(&env);

    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);

    bond.initialize(
        &issuer,
        &1_000_000_i128,
        &1_900_000_000_u64,
        &500_u32,
        &String::from_str(&env, "FRRBD00001"),
        &mock_whitelist,
    );
    // Second call must panic
    bond.initialize(
        &issuer,
        &1_000_000_i128,
        &1_900_000_000_u64,
        &500_u32,
        &String::from_str(&env, "FRRBD00001"),
        &mock_whitelist,
    );
}

// ---------------------------------------------------------------------------
// Balance tests
// ---------------------------------------------------------------------------

#[test]
fn test_balance_defaults_to_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let mock_whitelist = Address::generate(&env);

    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let random = Address::generate(&env);

    bond.initialize(
        &issuer,
        &1_000_000_i128,
        &1_900_000_000_u64,
        &500_u32,
        &String::from_str(&env, "FRRBD00001"),
        &mock_whitelist,
    );

    assert_eq!(bond.balance(&random), 0);
}

// ---------------------------------------------------------------------------
// Note: mint / transfer tests require the KYC Whitelist contract to be
// deployed in the same test environment. Integration tests live in
// tests/integration.rs once both contracts are registered.
// ---------------------------------------------------------------------------
