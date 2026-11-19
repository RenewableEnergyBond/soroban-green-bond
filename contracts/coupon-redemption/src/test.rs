#![cfg(test)]

use super::*;
use green_bond::{BondConfig, GreenBondContract, GreenBondContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal, Vec,
};

fn setup_with_contract() -> (
    Env,
    CouponRedemptionContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let usdc_asset = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let settlement_asset = usdc_asset.address();
    let bond_contract = deploy_bond(&env, &issuer, &settlement_asset);

    client.initialize(&issuer, &bond_contract, &settlement_asset);

    (
        env,
        client,
        issuer,
        bond_contract,
        settlement_asset,
        contract_id,
    )
}

fn setup() -> (
    Env,
    CouponRedemptionContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let (env, client, issuer, bond_contract, settlement_asset, _) = setup_with_contract();
    (env, client, issuer, bond_contract, settlement_asset)
}

fn deploy_bond(env: &Env, issuer: &Address, settlement_asset: &Address) -> Address {
    let whitelist = Address::generate(env);
    let bond_id = env.register(GreenBondContract, ());
    let bond = GreenBondContractClient::new(env, &bond_id);
    bond.initialize(&BondConfig {
        issuer: issuer.clone(),
        total_supply: 1_000_000_i128,
        maturity_timestamp: 1_900_000_000_u64,
        coupon_rate_bps: 500_u32,
        coupon_periods_per_year: 4_u32,
        settlement_asset: settlement_asset.clone(),
        isin: soroban_sdk::String::from_str(env, "FRRBD00001"),
        name: soroban_sdk::String::from_str(env, "Rebond Green Bond 2032"),
        symbol: soroban_sdk::String::from_str(env, "RGB32"),
        decimals: 0_u32,
        whitelist_contract: whitelist,
    });
    bond_id
}

fn create_test_campaign(
    env: &Env,
    client: &CouponRedemptionContractClient,
    merkle_root: &BytesN<32>,
    total_amount: i128,
) -> (u32, u32) {
    const PERIOD_END: u64 = 1_800_000_000;
    const PERIOD_SECONDS: u64 = 90 * 24 * 60 * 60;
    env.ledger().set_timestamp(PERIOD_END);
    let record_ledger = env.ledger().sequence();
    let deadline = record_ledger + 10;
    let coupon_id = client.create_coupon(
        &record_ledger,
        &(PERIOD_END - PERIOD_SECONDS),
        &PERIOD_END,
        &deadline,
        merkle_root,
        &total_amount,
    );
    (coupon_id, deadline)
}

#[test]
fn test_initialize_stores_addresses() {
    let (_, client, _, bond_contract, settlement_asset) = setup();
    assert_eq!(client.get_bond_contract(), bond_contract);
    assert_eq!(client.get_settlement_asset(), settlement_asset);
    assert_eq!(client.next_coupon_id(), 0);
}

#[test]
#[should_panic(expected = "settlement asset does not match bond")]
fn test_initialize_rejects_different_settlement_asset_than_bond() {
    let env = Env::default();
    env.mock_all_auths();
    let issuer = Address::generate(&env);
    let bond_asset = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let different_asset = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let bond_id = deploy_bond(&env, &issuer, &bond_asset.address());
    let coupon_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &coupon_id);

    client.initialize(&issuer, &bond_id, &different_asset.address());
}

#[test]
#[should_panic]
fn test_initialize_rejects_non_token_settlement_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let issuer = Address::generate(&env);
    let invalid_asset = Address::generate(&env);
    let bond_id = deploy_bond(&env, &issuer, &invalid_asset);
    let coupon_id = env.register(CouponRedemptionContract, ());
    let client = CouponRedemptionContractClient::new(&env, &coupon_id);

    client.initialize(&issuer, &bond_id, &invalid_asset);
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
    let (env, client, issuer, _, settlement_asset, contract_id) = setup_with_contract();
    StellarAssetClient::new(&env, &settlement_asset).mint(&contract_id, &1_000_i128);

    let holder = Address::generate(&env);
    let leaf = CouponLeaf {
        coupon_id: 0,
        holder: holder.clone(),
        amount: 600,
    };
    let root = hash_leaf(&env, &leaf);
    let (created_id, deadline) = create_test_campaign(&env, &client, &root, 1_000_i128);
    assert_eq!(created_id, 0);

    let proof: Vec<soroban_sdk::BytesN<32>> = Vec::new(&env);
    env.mock_auths(&[MockAuth {
        address: &holder,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "claim_coupon",
            args: (&created_id, &holder, &600_i128, &proof).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.claim_coupon(&created_id, &holder, &600_i128, &proof);

    let settlement_token = TokenClient::new(&env, &settlement_asset);
    assert_eq!(settlement_token.balance(&holder), 600);
    assert_eq!(client.reserved_settlement_amount(), 400);
    assert_eq!(client.get_coupon(&created_id).claimed_amount, 600);

    env.ledger().set_sequence_number(deadline + 1);
    env.mock_auths(&[MockAuth {
        address: &issuer,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "close_coupon",
            args: (&created_id,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.close_coupon(&created_id);

    assert_eq!(settlement_token.balance(&issuer), 400);
    assert_eq!(client.reserved_settlement_amount(), 0);
}

#[test]
#[should_panic(expected = "coupon already claimed")]
fn test_coupon_claim_cannot_be_replayed() {
    let (env, client, _, _, settlement_asset, contract_id) = setup_with_contract();
    StellarAssetClient::new(&env, &settlement_asset).mint(&contract_id, &100_i128);

    let holder = Address::generate(&env);
    let leaf = CouponLeaf {
        coupon_id: 0,
        holder: holder.clone(),
        amount: 100,
    };
    let root = hash_leaf(&env, &leaf);
    let (created_id, _) = create_test_campaign(&env, &client, &root, 100_i128);
    let proof: Vec<soroban_sdk::BytesN<32>> = Vec::new(&env);

    client.claim_coupon(&created_id, &holder, &100_i128, &proof);
    client.claim_coupon(&created_id, &holder, &100_i128, &proof);
}

#[test]
fn test_merkle_claim_supports_multiple_leaves() {
    let (env, client, _, _, settlement_asset, contract_id) = setup_with_contract();
    StellarAssetClient::new(&env, &settlement_asset).mint(&contract_id, &1_000_i128);

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
    let (coupon_id, _) = create_test_campaign(&env, &client, &root, 1_000_i128);
    let mut proof_a = Vec::new(&env);
    proof_a.push_back(hash_b);

    client.claim_coupon(&coupon_id, &holder_a, &400_i128, &proof_a);

    assert_eq!(
        TokenClient::new(&env, &settlement_asset).balance(&holder_a),
        400
    );
    assert_eq!(client.get_coupon(&coupon_id).claimed_amount, 400);
}

#[test]
#[should_panic(expected = "invalid Merkle proof")]
fn test_coupon_claim_rejects_invalid_proof() {
    let (env, client, _, _, settlement_asset, contract_id) = setup_with_contract();
    StellarAssetClient::new(&env, &settlement_asset).mint(&contract_id, &100_i128);

    let holder = Address::generate(&env);
    let leaf = CouponLeaf {
        coupon_id: 0,
        holder: holder.clone(),
        amount: 100,
    };
    let root = hash_leaf(&env, &leaf);
    let (coupon_id, _) = create_test_campaign(&env, &client, &root, 100_i128);
    let proof: Vec<soroban_sdk::BytesN<32>> = Vec::new(&env);

    client.claim_coupon(&coupon_id, &holder, &99_i128, &proof);
}

#[test]
#[should_panic]
fn test_coupon_claim_requires_holder_auth() {
    let (env, client, _, _, settlement_asset, contract_id) = setup_with_contract();
    StellarAssetClient::new(&env, &settlement_asset).mint(&contract_id, &100_i128);

    let holder = Address::generate(&env);
    let leaf = CouponLeaf {
        coupon_id: 0,
        holder: holder.clone(),
        amount: 100,
    };
    let root = hash_leaf(&env, &leaf);
    let (coupon_id, _) = create_test_campaign(&env, &client, &root, 100_i128);
    let proof: Vec<soroban_sdk::BytesN<32>> = Vec::new(&env);
    env.mock_auths(&[]);

    client.claim_coupon(&coupon_id, &holder, &100_i128, &proof);
}

#[test]
#[should_panic(expected = "insufficient settlement funding")]
fn test_coupon_campaign_requires_unreserved_funding() {
    let (env, client, _, _, settlement_asset, contract_id) = setup_with_contract();
    StellarAssetClient::new(&env, &settlement_asset).mint(&contract_id, &100_i128);

    let root = soroban_sdk::BytesN::from_array(&env, &[0; 32]);
    create_test_campaign(&env, &client, &root, 101_i128);
}

#[test]
#[should_panic]
fn test_create_coupon_requires_issuer_auth() {
    let (env, client, _, _, settlement_asset, contract_id) = setup_with_contract();
    StellarAssetClient::new(&env, &settlement_asset).mint(&contract_id, &100_i128);
    env.mock_auths(&[]);

    let root = soroban_sdk::BytesN::from_array(&env, &[0; 32]);
    create_test_campaign(&env, &client, &root, 100_i128);
}
