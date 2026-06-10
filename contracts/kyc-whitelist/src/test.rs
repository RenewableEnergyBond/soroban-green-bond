#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, KycWhitelistContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, KycWhitelistContract);
    let client = KycWhitelistContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    (env, client, admin)
}

// ---------------------------------------------------------------------------
// Initialisation tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_sets_admin() {
    let (_, client, admin) = setup();
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.whitelist_count(), 0);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_double_initialize_panics() {
    let (env, client, _) = setup();
    let other_admin = Address::generate(&env);
    client.initialize(&other_admin);
}

// ---------------------------------------------------------------------------
// Whitelist management tests
// ---------------------------------------------------------------------------

#[test]
fn test_add_investor_marks_whitelisted() {
    let (env, client, _) = setup();
    let investor = Address::generate(&env);

    assert!(!client.is_wl(&investor));
    client.add(&investor);
    assert!(client.is_wl(&investor));
    assert_eq!(client.whitelist_count(), 1);
}

#[test]
fn test_add_duplicate_does_not_increment_count() {
    let (env, client, _) = setup();
    let investor = Address::generate(&env);

    client.add(&investor);
    client.add(&investor); // second call — no-op
    assert_eq!(client.whitelist_count(), 1);
}

#[test]
fn test_remove_investor_marks_not_whitelisted() {
    let (env, client, _) = setup();
    let investor = Address::generate(&env);

    client.add(&investor);
    assert!(client.is_wl(&investor));

    client.remove(&investor);
    assert!(!client.is_wl(&investor));
    assert_eq!(client.whitelist_count(), 0);
}

#[test]
fn test_remove_absent_address_is_noop() {
    let (env, client, _) = setup();
    let random = Address::generate(&env);

    // Should not panic
    client.remove(&random);
    assert_eq!(client.whitelist_count(), 0);
}

#[test]
fn test_multiple_investors_tracked() {
    let (env, client, _) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    client.add(&a);
    client.add(&b);
    client.add(&c);
    assert_eq!(client.whitelist_count(), 3);

    client.remove(&b);
    assert_eq!(client.whitelist_count(), 2);
    assert!(client.is_wl(&a));
    assert!(!client.is_wl(&b));
    assert!(client.is_wl(&c));
}

// ---------------------------------------------------------------------------
// Admin transfer tests
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_admin() {
    let (env, client, _) = setup();
    let new_admin = Address::generate(&env);

    client.transfer_admin(&new_admin);
    assert_eq!(client.get_admin(), new_admin);
}
