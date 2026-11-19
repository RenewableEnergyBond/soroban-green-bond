#![cfg(test)]

use super::*;
use kyc_whitelist::{KycWhitelistContract, KycWhitelistContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::TokenClient,
    Address, Env, String,
};

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
    bond.initialize(&BondConfig {
        issuer: issuer.clone(),
        total_supply: 1_000_000_i128,
        maturity_timestamp: 1_900_000_000_u64,
        coupon_rate_bps: 500_u32,
        coupon_periods_per_year: 4_u32,
        settlement_asset: Address::generate(env),
        isin: String::from_str(env, "FRRBD00001"),
        name: String::from_str(env, "Rebond Green Bond 2032"),
        symbol: String::from_str(env, "RGB32"),
        decimals: 0_u32,
        whitelist_contract: whitelist.clone(),
    });
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
    assert_eq!(info.lifetime_issued_supply, 0);
    assert_eq!(info.maturity_timestamp, 1_900_000_000);
    assert_eq!(info.coupon_rate_bps, 500);
    assert_eq!(info.coupon_periods_per_year, 4);
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

#[test]
#[should_panic]
fn test_initialize_requires_issuer_auth() {
    let env = Env::default();
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let whitelist = Address::generate(&env);

    bond.initialize(&BondConfig {
        issuer,
        total_supply: 1_000_i128,
        maturity_timestamp: 1_900_000_000_u64,
        coupon_rate_bps: 500_u32,
        coupon_periods_per_year: 4_u32,
        settlement_asset: Address::generate(&env),
        isin: String::from_str(&env, "FRRBD00001"),
        name: String::from_str(&env, "Rebond Green Bond 2032"),
        symbol: String::from_str(&env, "RGB32"),
        decimals: 0_u32,
        whitelist_contract: whitelist,
    });
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

    assert_eq!(
        bond.name(),
        String::from_str(&env, "Rebond Green Bond 2032")
    );
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
#[should_panic]
fn test_mint_requires_issuer_auth() {
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
    env.mock_auths(&[]);

    bond.mint(&holder, &100_i128);
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
    let info = bond.get_bond_info();
    assert_eq!(info.minted_supply, 600);
    assert_eq!(info.lifetime_issued_supply, 1_000);
}

#[test]
#[should_panic(expected = "exceeds total_supply cap")]
fn test_burn_does_not_restore_lifetime_issuance_capacity() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (wl_id, wl) = deploy_whitelist(&env, &admin);
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let holder = Address::generate(&env);

    bond.initialize(&BondConfig {
        issuer,
        total_supply: 1_000_i128,
        maturity_timestamp: 1_900_000_000_u64,
        coupon_rate_bps: 500_u32,
        coupon_periods_per_year: 4_u32,
        settlement_asset: Address::generate(&env),
        isin: String::from_str(&env, "FRRBD00001"),
        name: String::from_str(&env, "Rebond Green Bond 2032"),
        symbol: String::from_str(&env, "RGB32"),
        decimals: 0_u32,
        whitelist_contract: wl_id,
    });

    wl.add(&holder);
    bond.mint(&holder, &1_000_i128);
    bond.burn(&holder, &400_i128);

    // The lifetime issuance cap remains exhausted even though 400 tokens
    // are no longer outstanding.
    bond.mint(&holder, &400_i128);
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

#[test]
fn test_allowance_can_be_replaced_and_revoked() {
    let env = Env::default();
    env.mock_all_auths();
    let whitelist = Address::generate(&env);
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let holder = Address::generate(&env);
    let spender = Address::generate(&env);
    let issuer = Address::generate(&env);
    init_bond(&env, &bond, &issuer, &whitelist);

    let current_ledger = env.ledger().sequence();
    bond.approve(&holder, &spender, &500_i128, &(current_ledger + 100));
    assert_eq!(bond.allowance(&holder, &spender), 500);

    bond.approve(&holder, &spender, &200_i128, &(current_ledger + 200));
    assert_eq!(bond.allowance(&holder, &spender), 200);

    bond.approve(&holder, &spender, &0_i128, &0_u32);
    assert_eq!(bond.allowance(&holder, &spender), 0);
}

#[test]
#[should_panic]
fn test_approve_requires_holder_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let whitelist = Address::generate(&env);
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let holder = Address::generate(&env);
    let spender = Address::generate(&env);
    init_bond(&env, &bond, &issuer, &whitelist);
    env.mock_auths(&[]);

    bond.approve(
        &holder,
        &spender,
        &100_i128,
        &(env.ledger().sequence() + 10),
    );
}

#[test]
fn test_expired_allowance_is_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let whitelist = Address::generate(&env);
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let holder = Address::generate(&env);
    let spender = Address::generate(&env);
    let issuer = Address::generate(&env);
    init_bond(&env, &bond, &issuer, &whitelist);

    let expiry = env.ledger().sequence() + 1;
    bond.approve(&holder, &spender, &100_i128, &expiry);
    env.ledger().set_sequence_number(expiry + 1);

    assert_eq!(bond.allowance(&holder, &spender), 0);
}

#[test]
fn test_transfer_from_consumes_allowance() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (wl_id, wl) = deploy_whitelist(&env, &admin);
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let holder = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);
    init_bond(&env, &bond, &issuer, &wl_id);

    wl.add(&holder);
    wl.add(&recipient);
    bond.mint(&holder, &1_000_i128);
    let expiry = env.ledger().sequence() + 100;
    bond.approve(&holder, &spender, &600_i128, &expiry);

    bond.transfer_from(&spender, &holder, &recipient, &400_i128);

    assert_eq!(bond.balance(&holder), 600);
    assert_eq!(bond.balance(&recipient), 400);
    assert_eq!(bond.allowance(&holder, &spender), 200);
}

#[test]
#[should_panic]
fn test_transfer_from_requires_spender_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (wl_id, wl) = deploy_whitelist(&env, &admin);
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let holder = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);
    init_bond(&env, &bond, &issuer, &wl_id);
    wl.add(&holder);
    wl.add(&recipient);
    bond.mint(&holder, &100_i128);
    let expiry = env.ledger().sequence() + 100;
    bond.approve(&holder, &spender, &100_i128, &expiry);
    env.mock_auths(&[]);

    bond.transfer_from(&spender, &holder, &recipient, &100_i128);
}

#[test]
fn test_burn_from_consumes_allowance_without_restoring_lifetime_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (wl_id, wl) = deploy_whitelist(&env, &admin);
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let holder = Address::generate(&env);
    let spender = Address::generate(&env);
    init_bond(&env, &bond, &issuer, &wl_id);

    wl.add(&holder);
    bond.mint(&holder, &1_000_i128);
    let expiry = env.ledger().sequence() + 100;
    bond.approve(&holder, &spender, &500_i128, &expiry);

    bond.burn_from(&spender, &holder, &400_i128);

    assert_eq!(bond.balance(&holder), 600);
    assert_eq!(bond.allowance(&holder, &spender), 100);
    let info = bond.get_bond_info();
    assert_eq!(info.minted_supply, 600);
    assert_eq!(info.lifetime_issued_supply, 1_000);
}

#[test]
fn test_official_sep41_token_client_can_use_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (wl_id, wl) = deploy_whitelist(&env, &admin);
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let token = TokenClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let holder = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);
    init_bond(&env, &bond, &issuer, &wl_id);

    wl.add(&holder);
    wl.add(&recipient);
    bond.mint(&holder, &1_000_i128);

    assert_eq!(
        token.name(),
        String::from_str(&env, "Rebond Green Bond 2032")
    );
    assert_eq!(token.symbol(), String::from_str(&env, "RGB32"));
    assert_eq!(token.decimals(), 0);
    assert_eq!(token.balance(&holder), 1_000);

    let expiry = env.ledger().sequence() + 100;
    token.approve(&holder, &spender, &300_i128, &expiry);
    token.transfer_from(&spender, &holder, &recipient, &250_i128);

    assert_eq!(token.allowance(&holder, &spender), 50);
    assert_eq!(token.balance(&recipient), 250);
}

#[test]
fn test_refresh_balance_ttl_reports_existing_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (wl_id, wl) = deploy_whitelist(&env, &admin);
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(&env, &bond_id);
    let issuer = Address::generate(&env);
    let holder = Address::generate(&env);
    let random = Address::generate(&env);
    init_bond(&env, &bond, &issuer, &wl_id);

    wl.add(&holder);
    bond.mint(&holder, &100_i128);

    assert!(bond.refresh_balance_ttl(&holder));
    assert!(!bond.refresh_balance_ttl(&random));
}
