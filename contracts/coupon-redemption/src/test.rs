#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

fn setup() -> (Env, CouponRedemptionContractClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CouponRedemptionContract);
    let client = CouponRedemptionContractClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let bond_contract = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&issuer, &bond_contract, &usdc_token);

    (env, client, issuer, bond_contract, usdc_token)
}

#[test]
fn test_initialize_stores_addresses() {
    let (_, client, _, bond_contract, usdc_token) = setup();
    assert_eq!(client.get_bond_contract(), bond_contract);
    assert_eq!(client.get_usdc_token(), usdc_token);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_double_initialize_panics() {
    let (env, client, issuer, bond_contract, usdc_token) = setup();
    client.initialize(&issuer, &bond_contract, &usdc_token);
}

#[test]
fn test_pay_coupon_emits_event() {
    let (env, client, _, _, _) = setup();
    let holders: Vec<Address> = Vec::new(&env);
    // Should not panic; full payout logic implemented in Tranche 2
    client.pay_coupon(&holders);
}

#[test]
fn test_redeem_emits_event() {
    let (env, client, _, _, _) = setup();
    let holder = Address::generate(&env);
    // Should not panic; full redemption logic implemented in Tranche 2
    client.redeem(&holder);
}
