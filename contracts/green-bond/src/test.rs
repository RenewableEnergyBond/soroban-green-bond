#![cfg(test)]

use super::*;
use kyc_whitelist::{KycWhitelistContract, KycWhitelistContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Deploy a real KYC Whitelist contract in the test env and return its client.
fn deploy_whitelist<'a>(env: &Env, admin: &Address) -> (Address, KycWhitelistContractClient<'a>) {
    let wl_id = env.register(KycWhitelistContract, ());
    let wl = KycWhitelistContractClient::new(env, &wl_id);
    wl.initialize(admin);
    (wl_id, wl)
}

/// Initialize a bond with the standard test parameters against `whitelist`.
fn init_bond(env: &Env, bond: &GreenBondContractClient, issuer: &Address, whitelist: &Address) {
    bond.initialize(
        issuer,
        &1_000_000_i128,
        &1_900_000_000_u64,
        &500_u32,
        &String::from_str(env, "FRRBD00001"),
        &String::from_str(env, "Rebond Green Bond 2032"),
        &String::from_str(env, "RGB32"),
        &0_u32,
        whitelist,
    );
}

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

    init_bond(&env, &bond, &issuer, &mock_whitelist);

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

    init_bond(&env, &bond, &issuer, &mock_whitelist);
    // Second call must panic
    init_bond(&env, &bond, &issuer, &mock_whitelist);
}

// ---------------------------------------------------------------------------
// SEP-41 metadata tests
// ---------------------------------------------------------------------------

#[test]
fn test_metadata_is_exposed() {
    let env = Env::default();
    env.mock_all_auths();
    let mock_whitelist = Address::generate(&env);

    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);

    init_bond(&env, &bond, &issuer, &mock_whitelist);

    assert_eq!(bond.name(), String::from_str(&env, "Rebond Green Bond 2032"));
    assert_eq!(bond.symbol(), String::from_str(&env, "RGB32"));
    assert_eq!(bond.decimals(), 0);
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

    init_bond(&env, &bond, &issuer, &mock_whitelist);

    assert_eq!(bond.balance(&random), 0);
}

// ---------------------------------------------------------------------------
// Mint tests (real KYC Whitelist cross-contract enforcement)
// ---------------------------------------------------------------------------

#[test]
fn test_mint_to_whitelisted_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (wl_id, wl) = deploy_whitelist(&env, &admin);

    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let investor = Address::generate(&env);

    init_bond(&env, &bond, &issuer, &wl_id);

    wl.add(&investor);
    bond.mint(&investor, &1_000_i128);

    assert_eq!(bond.balance(&investor), 1_000);
    assert_eq!(bond.get_bond_info().minted_supply, 1_000);
}

#[test]
#[should_panic(expected = "address not in KYC whitelist")]
fn test_mint_to_non_whitelisted_reverts() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (wl_id, _wl) = deploy_whitelist(&env, &admin);

    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let stranger = Address::generate(&env);

    init_bond(&env, &bond, &issuer, &wl_id);

    // `stranger` was never added to the whitelist → must revert.
    bond.mint(&stranger, &1_000_i128);
}

// ---------------------------------------------------------------------------
// Transfer tests — both parties must be whitelisted
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "address not in KYC whitelist")]
fn test_transfer_reverts_when_sender_not_whitelisted() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (wl_id, wl) = deploy_whitelist(&env, &admin);

    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let holder = Address::generate(&env);
    let recipient = Address::generate(&env);

    init_bond(&env, &bond, &issuer, &wl_id);

    // `holder` is whitelisted, receives tokens, then is de-listed.
    wl.add(&holder);
    wl.add(&recipient);
    bond.mint(&holder, &1_000_i128);
    wl.remove(&holder);

    // `holder` is no longer whitelisted → transfer must revert on the `from` check.
    bond.transfer(&holder, &recipient, &500_i128);
}

// ---------------------------------------------------------------------------
// Burn tests (redemption support)
// ---------------------------------------------------------------------------

#[test]
fn test_burn_reduces_balance_and_supply() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (wl_id, wl) = deploy_whitelist(&env, &admin);

    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let holder = Address::generate(&env);

    init_bond(&env, &bond, &issuer, &wl_id);

    wl.add(&holder);
    bond.mint(&holder, &1_000_i128);

    bond.burn(&holder, &400_i128);

    assert_eq!(bond.balance(&holder), 600);
    assert_eq!(bond.get_bond_info().minted_supply, 600);
}

#[test]
#[should_panic(expected = "insufficient balance")]
fn test_burn_more_than_balance_reverts() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (wl_id, wl) = deploy_whitelist(&env, &admin);

    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let holder = Address::generate(&env);

    init_bond(&env, &bond, &issuer, &wl_id);

    wl.add(&holder);
    bond.mint(&holder, &100_i128);

    // Burning more than the balance must revert.
    bond.burn(&holder, &500_i128);
}

