#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env, Vec,
};

fn setup() -> (
    Env,
    CouponRedemptionContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CouponRedemptionContract, ());
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
    let (_env, client, issuer, bond_contract, usdc_token) = setup();
    client.initialize(&issuer, &bond_contract, &usdc_token);
}

#[test]
#[should_panic]
fn test_initialize_requires_issuer_auth() {
    let env = Env::default();
    let contract_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let bond_contract = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&issuer, &bond_contract, &usdc_token);
}

#[test]
#[should_panic(expected = "not implemented, Tranche 2")]
fn test_pay_coupon_not_implemented_reverts() {
    let (env, client, _, _, _) = setup();
    let holders: Vec<Address> = Vec::new(&env);
    // pay_coupon must fail explicitly until the Tranche 2 payout logic lands,
    // rather than emit a success event for a payout that never happened.
    client.pay_coupon(&holders);
}

#[test]
#[should_panic(expected = "not implemented, Tranche 2")]
fn test_redeem_not_implemented_reverts() {
    let (env, client, _, _, _) = setup();
    let holder = Address::generate(&env);
    // redeem must fail explicitly until the Tranche 2 redemption logic lands.
    client.redeem(&holder);
}

#[test]
fn test_merkle_coupon_claim_pays_holder_and_refunds_unclaimed_usdc() {
    let env = Env::default();
    env.mock_all_auths();

    let coupon_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &coupon_id);
    let issuer = Address::generate(&env);
    let bond_contract = Address::generate(&env);
    let usdc_asset = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let usdc = usdc_asset.address();
    client.initialize(&issuer, &bond_contract, &usdc);

    let usdc_client = StellarAssetClient::new(&env, &usdc);
    usdc_client.mint(&coupon_id, &1_000_i128);

    let holder = Address::generate(&env);
    let leaf = CouponLeaf {
        coupon_id: 0,
        holder: holder.clone(),
        amount: 600,
    };
    let root = hash_leaf(&env, &leaf);
    let record_ledger = env.ledger().sequence();
    let deadline = record_ledger + 10;
    let created_id = client.create_coupon(&record_ledger, &deadline, &root, &1_000_i128);
    assert_eq!(created_id, 0);

    let proof: Vec<soroban_sdk::BytesN<32>> = Vec::new(&env);
    client.claim_coupon(&created_id, &holder, &600_i128, &proof);

    let usdc_token = TokenClient::new(&env, &usdc);
    assert_eq!(usdc_token.balance(&holder), 600);
    assert_eq!(client.reserved_usdc(), 400);
    assert_eq!(client.get_coupon(&created_id).claimed_amount, 600);

    env.ledger().set_sequence_number(deadline + 1);
    client.close_coupon(&created_id);

    assert_eq!(usdc_token.balance(&issuer), 400);
    assert_eq!(client.reserved_usdc(), 0);
}

#[test]
#[should_panic(expected = "coupon already claimed")]
fn test_coupon_claim_cannot_be_replayed() {
    let env = Env::default();
    env.mock_all_auths();

    let coupon_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &coupon_id);
    let issuer = Address::generate(&env);
    let bond_contract = Address::generate(&env);
    let usdc_asset = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let usdc = usdc_asset.address();
    client.initialize(&issuer, &bond_contract, &usdc);
    StellarAssetClient::new(&env, &usdc).mint(&coupon_id, &100_i128);

    let holder = Address::generate(&env);
    let leaf = CouponLeaf {
        coupon_id: 0,
        holder: holder.clone(),
        amount: 100,
    };
    let root = hash_leaf(&env, &leaf);
    let record_ledger = env.ledger().sequence();
    let deadline = record_ledger + 10;
    let created_id = client.create_coupon(&record_ledger, &deadline, &root, &100_i128);
    let proof: Vec<soroban_sdk::BytesN<32>> = Vec::new(&env);

    client.claim_coupon(&created_id, &holder, &100_i128, &proof);
    client.claim_coupon(&created_id, &holder, &100_i128, &proof);
}

#[test]
fn test_merkle_claim_supports_multiple_leaves() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let usdc = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let usdc_id = usdc.address();
    client.initialize(&issuer, &Address::generate(&env), &usdc_id);
    StellarAssetClient::new(&env, &usdc_id).mint(&contract_id, &1_000_i128);

    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);
    let leaf_a = CouponLeaf {
        coupon_id: 0,
        holder: holder_a.clone(),
        amount: 400,
    };
    let leaf_b = CouponLeaf {
        coupon_id: 0,
        holder: holder_b.clone(),
        amount: 600,
    };
    let hash_a = hash_leaf(&env, &leaf_a);
    let hash_b = hash_leaf(&env, &leaf_b);
    let root = hash_pair(&env, &hash_a, &hash_b);
    let record_ledger = env.ledger().sequence();
    let deadline = record_ledger + 10;
    let coupon_id = client.create_coupon(&record_ledger, &deadline, &root, &1_000_i128);
    let mut proof_a = Vec::new(&env);
    proof_a.push_back(hash_b);

    client.claim_coupon(&coupon_id, &holder_a, &400_i128, &proof_a);

    assert_eq!(TokenClient::new(&env, &usdc_id).balance(&holder_a), 400);
    assert_eq!(client.get_coupon(&coupon_id).claimed_amount, 400);
}

#[test]
#[should_panic(expected = "invalid Merkle proof")]
fn test_coupon_claim_rejects_invalid_proof() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let usdc = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let usdc_id = usdc.address();
    client.initialize(&issuer, &Address::generate(&env), &usdc_id);
    StellarAssetClient::new(&env, &usdc_id).mint(&contract_id, &100_i128);

    let holder = Address::generate(&env);
    let leaf = CouponLeaf {
        coupon_id: 0,
        holder: holder.clone(),
        amount: 100,
    };
    let root = hash_leaf(&env, &leaf);
    let record_ledger = env.ledger().sequence();
    let deadline = record_ledger + 10;
    let coupon_id = client.create_coupon(&record_ledger, &deadline, &root, &100_i128);
    let proof: Vec<soroban_sdk::BytesN<32>> = Vec::new(&env);

    client.claim_coupon(&coupon_id, &holder, &99_i128, &proof);
}

#[test]
#[should_panic]
fn test_coupon_claim_requires_holder_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let usdc = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let usdc_id = usdc.address();
    client.initialize(&issuer, &Address::generate(&env), &usdc_id);
    StellarAssetClient::new(&env, &usdc_id).mint(&contract_id, &100_i128);

    let holder = Address::generate(&env);
    let leaf = CouponLeaf {
        coupon_id: 0,
        holder: holder.clone(),
        amount: 100,
    };
    let root = hash_leaf(&env, &leaf);
    let record_ledger = env.ledger().sequence();
    let deadline = record_ledger + 10;
    let coupon_id = client.create_coupon(&record_ledger, &deadline, &root, &100_i128);
    let proof: Vec<soroban_sdk::BytesN<32>> = Vec::new(&env);
    env.mock_auths(&[]);

    client.claim_coupon(&coupon_id, &holder, &100_i128, &proof);
}

#[test]
#[should_panic(expected = "insufficient USDC funding")]
fn test_coupon_campaign_requires_unreserved_funding() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let usdc = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let usdc_id = usdc.address();
    client.initialize(&issuer, &Address::generate(&env), &usdc_id);
    StellarAssetClient::new(&env, &usdc_id).mint(&contract_id, &100_i128);

    let record_ledger = env.ledger().sequence();
    let deadline = record_ledger + 10;
    let root = soroban_sdk::BytesN::from_array(&env, &[0; 32]);
    client.create_coupon(&record_ledger, &deadline, &root, &101_i128);
}

#[test]
#[should_panic]
fn test_create_coupon_requires_issuer_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let usdc = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let usdc_id = usdc.address();
    client.initialize(&issuer, &Address::generate(&env), &usdc_id);
    StellarAssetClient::new(&env, &usdc_id).mint(&contract_id, &100_i128);
    env.mock_auths(&[]);

    let record_ledger = env.ledger().sequence();
    let deadline = record_ledger + 10;
    let root = soroban_sdk::BytesN::from_array(&env, &[0; 32]);
    client.create_coupon(&record_ledger, &deadline, &root, &100_i128);
}
