//! Rebond — Coupon & Redemption Contract
//!
//! Handles funded Merkle-based USDC coupon claims for Rebond green bonds.
//! Each holder claims individually; no full holder list is processed in one
//! transaction. Principal redemption remains disabled until its terms are set.
//!
//! Licensed under the MIT License.
//! https://github.com/RenewableEnergyBond/soroban-green-bond

#![no_std]

use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    contract, contractevent, contractimpl, contracttype,
    token::TokenClient,
    xdr::ToXdr,
    Address, Bytes, BytesN, Env, IntoVal, MuxedAddress, Symbol, Vec,
};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Address authorised to trigger coupon and redemption flows
    Issuer,
    /// Address of the Green Bond contract
    BondContract,
    /// Address of the settlement token's Stellar Asset Contract
    SettlementToken,
    /// Next coupon campaign identifier
    NextCouponId,
    /// Settlement tokens reserved for unclaimed coupon entitlements
    ReservedSettlementAmount,
    /// Coupon campaign stored by identifier
    Coupon(u32),
    /// Duplicate-claim guard by campaign and holder
    Claimed(u32, Address),
    /// Guards against double-initialisation
    Initialized,
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub struct CouponCampaign {
    pub record_ledger: u32,
    pub period_start_timestamp: u64,
    pub period_end_timestamp: u64,
    pub annual_rate_bps: u32,
    pub periods_per_year: u32,
    pub claim_deadline: u32,
    pub merkle_root: BytesN<32>,
    pub total_amount: i128,
    pub claimed_amount: i128,
    pub closed: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct BondCouponTerms {
    pub annual_rate_bps: u32,
    pub periods_per_year: u32,
    pub settlement_asset: Address,
}

#[contracttype]
#[derive(Clone)]
pub struct CouponLeaf {
    pub coupon_id: u32,
    pub holder: Address,
    pub amount: i128,
}

#[contractevent(topics = ["cr_initialized"], data_format = "vec")]
pub struct CouponRedemptionInitialized {
    pub issuer: Address,
    pub bond_contract: Address,
}

#[contractevent(topics = ["coupon_created"], data_format = "vec")]
pub struct CouponCreated {
    #[topic]
    pub coupon_id: u32,
    pub record_ledger: u32,
    pub period_start_timestamp: u64,
    pub period_end_timestamp: u64,
    pub annual_rate_bps: u32,
    pub periods_per_year: u32,
    pub claim_deadline: u32,
    pub total_amount: i128,
    pub merkle_root: BytesN<32>,
}

#[contractevent(topics = ["coupon_claimed"], data_format = "single-value")]
pub struct CouponClaimed {
    #[topic]
    pub coupon_id: u32,
    #[topic]
    pub holder: Address,
    pub amount: i128,
}

#[contractevent(topics = ["coupon_closed"], data_format = "single-value")]
pub struct CouponClosed {
    #[topic]
    pub coupon_id: u32,
    pub refunded_amount: i128,
}

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;
const MAX_ENTRY_TTL: u32 = 6_312_000;
const CAMPAIGN_TTL_THRESHOLD: u32 = MAX_ENTRY_TTL - DAY_IN_LEDGERS;
const MAX_CLAIM_WINDOW: u32 = MAX_ENTRY_TTL - DAY_IN_LEDGERS;
const MAX_MERKLE_PROOF_DEPTH: u32 = 32;

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct CouponRedemptionContract;

#[contractimpl]
impl CouponRedemptionContract {
    // -----------------------------------------------------------------------
    // Initialisation
    // -----------------------------------------------------------------------

    /// Initialise the coupon/redemption module.
    ///
    /// # Arguments
    /// * `issuer`        – Address authorised to trigger payouts.
    /// * `bond_contract` – Address of the Soroban Green Bond contract.
    /// * `settlement_asset` – Stellar Asset Contract address for the bond's currency.
    pub fn initialize(
        env: Env,
        issuer: Address,
        bond_contract: Address,
        settlement_asset: Address,
    ) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }
        issuer.require_auth();
        let terms: BondCouponTerms = env.invoke_contract(
            &bond_contract,
            &Symbol::new(&env, "get_coupon_terms"),
            Vec::new(&env),
        );
        assert!(
            terms.settlement_asset == settlement_asset,
            "settlement asset does not match bond"
        );
        TokenClient::new(&env, &settlement_asset).balance(&env.current_contract_address());
        env.storage().instance().set(&DataKey::Issuer, &issuer);
        env.storage()
            .instance()
            .set(&DataKey::BondContract, &bond_contract);
        env.storage()
            .instance()
            .set(&DataKey::SettlementToken, &settlement_asset);
        env.storage().instance().set(&DataKey::NextCouponId, &0_u32);
        env.storage()
            .instance()
            .set(&DataKey::ReservedSettlementAmount, &0_i128);
        env.storage().instance().set(&DataKey::Initialized, &true);
        extend_instance_ttl(&env);

        CouponRedemptionInitialized {
            issuer,
            bond_contract,
        }
        .publish(&env);
    }

    // -----------------------------------------------------------------------
    // Coupon payments
    // -----------------------------------------------------------------------

    /// Register a funded claim campaign for a record-ledger snapshot and coupon period.
    /// Amounts are calculated off-chain using the bond terms and Actual/365 Fixed.
    pub fn create_coupon(
        env: Env,
        record_ledger: u32,
        period_start_timestamp: u64,
        period_end_timestamp: u64,
        claim_deadline: u32,
        merkle_root: BytesN<32>,
        total_amount: i128,
    ) -> u32 {
        let issuer: Address = env
            .storage()
            .instance()
            .get(&DataKey::Issuer)
            .expect("not initialized");
        issuer.require_auth();

        assert!(total_amount > 0, "total_amount must be positive");
        let current_ledger = env.ledger().sequence();
        assert!(
            record_ledger <= current_ledger,
            "record ledger is in the future"
        );
        assert!(
            period_start_timestamp < period_end_timestamp,
            "coupon period is empty"
        );
        assert!(
            period_end_timestamp <= env.ledger().timestamp(),
            "coupon period ends in the future"
        );
        assert!(
            claim_deadline > current_ledger,
            "claim deadline must be in the future"
        );
        assert!(
            claim_deadline <= current_ledger.saturating_add(MAX_CLAIM_WINDOW),
            "claim deadline exceeds maximum TTL"
        );

        let coupon_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextCouponId)
            .unwrap_or(0);
        let reserved: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReservedSettlementAmount)
            .unwrap_or(0);
        let settlement_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::SettlementToken)
            .expect("not initialized");
        let token = TokenClient::new(&env, &settlement_token);
        let contract_address = env.current_contract_address();
        let new_reserved = reserved
            .checked_add(total_amount)
            .expect("reserved settlement amount overflow");
        assert!(
            token.balance(&contract_address) >= new_reserved,
            "insufficient settlement funding"
        );
        let terms: BondCouponTerms = env.invoke_contract(
            &env.storage()
                .instance()
                .get(&DataKey::BondContract)
                .expect("not initialized"),
            &Symbol::new(&env, "get_coupon_terms"),
            Vec::new(&env),
        );

        let key = DataKey::Coupon(coupon_id);
        env.storage().persistent().set(
            &key,
            &CouponCampaign {
                record_ledger,
                period_start_timestamp,
                period_end_timestamp,
                annual_rate_bps: terms.annual_rate_bps,
                periods_per_year: terms.periods_per_year,
                claim_deadline,
                merkle_root: merkle_root.clone(),
                total_amount,
                claimed_amount: 0,
                closed: false,
            },
        );
        extend_campaign_ttl(&env, &key);
        env.storage().instance().set(
            &DataKey::NextCouponId,
            &coupon_id.checked_add(1).expect("coupon ID overflow"),
        );
        env.storage()
            .instance()
            .set(&DataKey::ReservedSettlementAmount, &new_reserved);
        extend_instance_ttl(&env);

        CouponCreated {
            coupon_id,
            record_ledger,
            period_start_timestamp,
            period_end_timestamp,
            annual_rate_bps: terms.annual_rate_bps,
            periods_per_year: terms.periods_per_year,
            claim_deadline,
            total_amount,
            merkle_root,
        }
        .publish(&env);

        coupon_id
    }

    /// Claim one holder's coupon amount using a proof against the campaign root.
    pub fn claim_coupon(
        env: Env,
        coupon_id: u32,
        holder: Address,
        amount: i128,
        proof: Vec<BytesN<32>>,
    ) {
        holder.require_auth();
        assert!(amount > 0, "amount must be positive");
        assert!(
            proof.len() <= MAX_MERKLE_PROOF_DEPTH,
            "Merkle proof is too deep"
        );

        let coupon_key = DataKey::Coupon(coupon_id);
        let mut campaign: CouponCampaign = env
            .storage()
            .persistent()
            .get(&coupon_key)
            .expect("coupon campaign not found");
        let current_ledger = env.ledger().sequence();
        assert!(
            current_ledger <= campaign.claim_deadline,
            "claim window closed"
        );
        assert!(!campaign.closed, "coupon campaign closed");

        let claimed_key = DataKey::Claimed(coupon_id, holder.clone());
        assert!(
            !env.storage().persistent().has(&claimed_key),
            "coupon already claimed"
        );

        let leaf = CouponLeaf {
            coupon_id,
            holder: holder.clone(),
            amount,
        };
        let mut hash = hash_leaf(&env, &leaf);
        for sibling in proof.iter() {
            hash = hash_pair(&env, &hash, &sibling);
        }
        assert!(hash == campaign.merkle_root, "invalid Merkle proof");
        let claimed_amount = campaign
            .claimed_amount
            .checked_add(amount)
            .expect("claimed amount overflow");
        assert!(
            claimed_amount <= campaign.total_amount,
            "claims exceed campaign funding"
        );

        campaign.claimed_amount = claimed_amount;
        env.storage().persistent().set(&coupon_key, &campaign);
        env.storage().persistent().set(&claimed_key, &true);
        extend_campaign_ttl(&env, &coupon_key);
        extend_campaign_ttl(&env, &claimed_key);

        let reserved: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReservedSettlementAmount)
            .unwrap_or(0);
        let new_reserved = reserved
            .checked_sub(amount)
            .expect("reserved settlement amount underflow");
        env.storage()
            .instance()
            .set(&DataKey::ReservedSettlementAmount, &new_reserved);
        extend_instance_ttl(&env);

        let settlement_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::SettlementToken)
            .expect("not initialized");
        authorize_settlement_transfer(&env, &settlement_token, &holder, amount);
        TokenClient::new(&env, &settlement_token).transfer(
            &env.current_contract_address(),
            MuxedAddress::from(holder.clone()),
            &amount,
        );

        CouponClaimed {
            coupon_id,
            holder,
            amount,
        }
        .publish(&env);
    }

    /// Return unclaimed campaign funds to the issuer after the claim deadline.
    pub fn close_coupon(env: Env, coupon_id: u32) {
        let issuer: Address = env
            .storage()
            .instance()
            .get(&DataKey::Issuer)
            .expect("not initialized");
        issuer.require_auth();

        let coupon_key = DataKey::Coupon(coupon_id);
        let mut campaign: CouponCampaign = env
            .storage()
            .persistent()
            .get(&coupon_key)
            .expect("coupon campaign not found");
        assert!(
            env.ledger().sequence() > campaign.claim_deadline,
            "claim window still open"
        );
        assert!(!campaign.closed, "coupon campaign closed");

        let refund_amount = campaign.total_amount - campaign.claimed_amount;
        campaign.closed = true;
        env.storage().persistent().set(&coupon_key, &campaign);
        extend_campaign_ttl(&env, &coupon_key);

        let reserved: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReservedSettlementAmount)
            .unwrap_or(0);
        env.storage().instance().set(
            &DataKey::ReservedSettlementAmount,
            &reserved
                .checked_sub(refund_amount)
                .expect("reserved settlement amount underflow"),
        );
        extend_instance_ttl(&env);

        if refund_amount > 0 {
            let settlement_token: Address = env
                .storage()
                .instance()
                .get(&DataKey::SettlementToken)
                .expect("not initialized");
            authorize_settlement_transfer(&env, &settlement_token, &issuer, refund_amount);
            TokenClient::new(&env, &settlement_token).transfer(
                &env.current_contract_address(),
                MuxedAddress::from(issuer),
                &refund_amount,
            );
        }

        CouponClosed {
            coupon_id,
            refunded_amount: refund_amount,
        }
        .publish(&env);
    }

    /// Disabled legacy batch entry point. Use a funded Merkle campaign and
    /// per-holder `claim_coupon` calls instead.
    pub fn pay_coupon(_env: Env, _holder_addresses: Vec<Address>) {
        panic!("not implemented, Tranche 2");
    }

    // -----------------------------------------------------------------------
    // Redemption at maturity
    // -----------------------------------------------------------------------

    /// Principal redemption is disabled until maturity and settlement rules
    /// are finalized. Fails explicitly instead of suggesting a successful payout.
    pub fn redeem(_env: Env, _holder: Address) {
        panic!("not implemented, Tranche 2");
    }

    // -----------------------------------------------------------------------
    // View functions
    // -----------------------------------------------------------------------

    /// Returns the address of the bond contract this module is linked to.
    pub fn get_bond_contract(env: Env) -> Address {
        extend_instance_ttl(&env);
        env.storage()
            .instance()
            .get(&DataKey::BondContract)
            .expect("not initialized")
    }

    /// Returns the bond's settlement token SAC address.
    pub fn get_settlement_asset(env: Env) -> Address {
        extend_instance_ttl(&env);
        env.storage()
            .instance()
            .get(&DataKey::SettlementToken)
            .expect("not initialized")
    }

    pub fn get_coupon(env: Env, coupon_id: u32) -> CouponCampaign {
        extend_instance_ttl(&env);
        let key = DataKey::Coupon(coupon_id);
        let campaign = env
            .storage()
            .persistent()
            .get(&key)
            .expect("coupon campaign not found");
        extend_campaign_ttl(&env, &key);
        campaign
    }

    pub fn reserved_settlement_amount(env: Env) -> i128 {
        extend_instance_ttl(&env);
        env.storage()
            .instance()
            .get(&DataKey::ReservedSettlementAmount)
            .unwrap_or(0)
    }

    pub fn next_coupon_id(env: Env) -> u32 {
        extend_instance_ttl(&env);
        env.storage()
            .instance()
            .get(&DataKey::NextCouponId)
            .unwrap_or(0)
    }

    /// Permissionless keeper entry point for extending instance-storage TTL.
    pub fn refresh_ttl(env: Env) {
        extend_instance_ttl(&env);
    }
}

fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

fn extend_campaign_ttl(env: &Env, key: &DataKey) {
    if env.storage().persistent().has(key) {
        env.storage()
            .persistent()
            .extend_ttl(key, CAMPAIGN_TTL_THRESHOLD, MAX_ENTRY_TTL);
    }
}

fn authorize_settlement_transfer(env: &Env, token: &Address, to: &Address, amount: i128) {
    let contract_address = env.current_contract_address();
    env.authorize_as_current_contract(Vec::from_array(
        env,
        [InvokerContractAuthEntry::Contract(SubContractInvocation {
            context: ContractContext {
                contract: token.clone(),
                fn_name: Symbol::new(env, "transfer"),
                args: Vec::from_array(
                    env,
                    [
                        contract_address.into_val(env),
                        MuxedAddress::from(to.clone()).into_val(env),
                        amount.into_val(env),
                    ],
                ),
            },
            sub_invocations: Vec::new(env),
        })],
    ));
}

pub fn hash_leaf(env: &Env, leaf: &CouponLeaf) -> BytesN<32> {
    let mut preimage = Bytes::new(env);
    preimage.push_back(0);
    preimage.append(&leaf.clone().to_xdr(env));
    env.crypto().sha256(&preimage).into()
}

pub fn hash_pair(env: &Env, left: &BytesN<32>, right: &BytesN<32>) -> BytesN<32> {
    let left = left.to_array();
    let right = right.to_array();
    let (first, second) = if left <= right {
        (left, right)
    } else {
        (right, left)
    };
    let mut preimage = [0_u8; 65];
    preimage[0] = 1;
    preimage[1..33].copy_from_slice(&first);
    preimage[33..].copy_from_slice(&second);
    env.crypto()
        .sha256(&Bytes::from_array(env, &preimage))
        .into()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test;
