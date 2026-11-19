//! Rebond — Soroban Green Bond Contract
//!
//! A MiFID II-aligned security token representing a green bond issued by a
//! renewable energy IPP. Bond parameters (total supply, maturity date, coupon
//! rate, ISIN) are immutably stored on-chain at issuance.
//!
//! Transfer restrictions are enforced via the KYC Whitelist contract: only
//! addresses registered in the whitelist can hold or receive bond tokens.
//!
//! Licensed under the MIT License.
//! https://github.com/RenewableEnergyBond/soroban-green-bond

#![no_std]

use soroban_sdk::{
    contract, contractevent, contractimpl, contracttype, symbol_short, token::TokenInterface,
    Address, Env, MuxedAddress, String, Val, Vec,
};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Address authorised to issue and manage the bond
    Issuer,
    /// Maximum number of tokens that can ever be minted
    TotalSupply,
    /// Tokens currently outstanding (legacy storage key name)
    MintedSupply,
    /// Tokens ever issued; burns never decrease this value
    LifetimeIssuedSupply,
    /// Unix timestamp (seconds) at which the bond matures
    MaturityTimestamp,
    /// Annual coupon rate in basis points (e.g. 500 = 5.00 %)
    CouponRateBps,
    /// Coupon periods per year: 1, 2, or 4
    CouponPeriodsPerYear,
    /// SAC address for the bond's settlement currency
    SettlementAsset,
    /// ISIN-equivalent identifier stored as a Symbol
    Isin,
    /// Human-readable token name (SEP-41 metadata)
    Name,
    /// Token symbol / ticker (SEP-41 metadata)
    TokenSymbol,
    /// Token decimals (SEP-41 metadata)
    Decimals,
    /// Address of the KYC Whitelist contract
    WhitelistContract,
    /// Per-investor token balance
    Balance(Address),
    /// Spending allowance keyed by holder and spender
    Allowance(Address, Address),
    /// Guards against double-initialisation
    Initialized,
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// Immutable bond parameters returned by `get_bond_info`.
#[contracttype]
#[derive(Clone)]
pub struct BondInfo {
    pub issuer: Address,
    pub total_supply: i128,
    /// Current outstanding supply (retained for compatibility with the prior field name).
    pub minted_supply: i128,
    pub lifetime_issued_supply: i128,
    pub maturity_timestamp: u64,
    pub coupon_rate_bps: u32,
    pub coupon_periods_per_year: u32,
    pub settlement_asset: Address,
    pub isin: String,
    pub whitelist_contract: Address,
}

#[contracttype]
#[derive(Clone)]
pub struct BondConfig {
    pub issuer: Address,
    pub total_supply: i128,
    pub maturity_timestamp: u64,
    pub coupon_rate_bps: u32,
    pub coupon_periods_per_year: u32,
    pub settlement_asset: Address,
    pub isin: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u32,
    pub whitelist_contract: Address,
}

#[contracttype]
#[derive(Clone)]
pub struct CouponTerms {
    pub annual_rate_bps: u32,
    pub periods_per_year: u32,
    pub settlement_asset: Address,
}

#[contracttype]
#[derive(Clone)]
pub struct AllowanceData {
    pub amount: i128,
    pub live_until_ledger: u32,
}

#[contractevent(topics = ["bond_initialized"], data_format = "vec")]
pub struct BondInitialized {
    pub issuer: Address,
    pub total_supply: i128,
    pub maturity_timestamp: u64,
    pub coupon_rate_bps: u32,
    pub coupon_periods_per_year: u32,
    pub settlement_asset: Address,
}

#[contractevent(data_format = "single-value")]
pub struct Mint {
    #[topic]
    pub to: Address,
    pub amount: i128,
}

#[contractevent(data_format = "map")]
pub struct Transfer {
    #[topic]
    pub from: Address,
    #[topic]
    pub to: Address,
    pub amount: i128,
    pub to_muxed_id: Option<u64>,
}

#[contractevent(data_format = "single-value")]
pub struct Burn {
    #[topic]
    pub from: Address,
    pub amount: i128,
}

#[contractevent(topics = ["approve"], data_format = "vec")]
pub struct Approve {
    #[topic]
    pub from: Address,
    #[topic]
    pub spender: Address,
    pub amount: i128,
    pub live_until_ledger: u32,
}

// ---------------------------------------------------------------------------
// Storage TTL (rent) management
// ---------------------------------------------------------------------------
//
// Soroban storage is rented, not owned: every entry has an expiration ledger
// and is archived if nobody extends it. This bond matures in 2032, so investor
// balances and whitelist entries must be periodically bumped to stay live.
// We extend the relevant entries on every interaction.

/// ~1 day of ledgers at a 5-second close time.
const DAY_IN_LEDGERS: u32 = 17_280;
/// Instance storage (bond parameters) is bumped ~30 days ahead on each call.
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;
/// Persistent entries (balances) are bumped ~90 days ahead on each touch.
const BALANCE_BUMP_AMOUNT: u32 = 90 * DAY_IN_LEDGERS;
const BALANCE_LIFETIME_THRESHOLD: u32 = BALANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;
/// Soroban permits persistent entries to remain live for at most 365 days.
const MAX_ENTRY_TTL: u32 = 6_312_000;
const ALLOWANCE_LIFETIME_THRESHOLD: u32 = MAX_ENTRY_TTL - DAY_IN_LEDGERS;

/// Extend the TTL of the instance storage (bond parameters).
fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Extend the TTL of a persistent balance entry, if it exists.
fn extend_balance_ttl(env: &Env, key: &DataKey) {
    if env.storage().persistent().has(key) {
        env.storage()
            .persistent()
            .extend_ttl(key, BALANCE_LIFETIME_THRESHOLD, BALANCE_BUMP_AMOUNT);
    }
}

fn extend_allowance_ttl(env: &Env, key: &DataKey) {
    if env.storage().persistent().has(key) {
        env.storage()
            .persistent()
            .extend_ttl(key, ALLOWANCE_LIFETIME_THRESHOLD, MAX_ENTRY_TTL);
    }
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct GreenBondContract;

#[contractimpl]
impl GreenBondContract {
    // -----------------------------------------------------------------------
    // Initialisation
    // -----------------------------------------------------------------------

    /// Initialise the bond. Can only be called once.
    ///
    /// Stores bond terms and SEP-41 metadata. Can only be called once.
    pub fn initialize(env: Env, config: BondConfig) {
        let BondConfig {
            issuer,
            total_supply,
            maturity_timestamp,
            coupon_rate_bps,
            coupon_periods_per_year,
            settlement_asset,
            isin,
            name,
            symbol,
            decimals,
            whitelist_contract,
        } = config;
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }
        issuer.require_auth();
        assert!(total_supply > 0, "total_supply must be positive");
        assert!(
            coupon_rate_bps <= 10_000,
            "coupon_rate_bps must be <= 10000"
        );
        assert!(
            coupon_periods_per_year == 1
                || coupon_periods_per_year == 2
                || coupon_periods_per_year == 4,
            "coupon periods per year must be 1, 2, or 4"
        );

        env.storage().instance().set(&DataKey::Issuer, &issuer);
        env.storage()
            .instance()
            .set(&DataKey::TotalSupply, &total_supply);
        env.storage()
            .instance()
            .set(&DataKey::MintedSupply, &0_i128);
        env.storage()
            .instance()
            .set(&DataKey::LifetimeIssuedSupply, &0_i128);
        env.storage()
            .instance()
            .set(&DataKey::MaturityTimestamp, &maturity_timestamp);
        env.storage()
            .instance()
            .set(&DataKey::CouponRateBps, &coupon_rate_bps);
        env.storage()
            .instance()
            .set(&DataKey::CouponPeriodsPerYear, &coupon_periods_per_year);
        env.storage()
            .instance()
            .set(&DataKey::SettlementAsset, &settlement_asset);
        env.storage().instance().set(&DataKey::Isin, &isin);
        env.storage().instance().set(&DataKey::Name, &name);
        env.storage().instance().set(&DataKey::TokenSymbol, &symbol);
        env.storage().instance().set(&DataKey::Decimals, &decimals);
        env.storage()
            .instance()
            .set(&DataKey::WhitelistContract, &whitelist_contract);
        env.storage().instance().set(&DataKey::Initialized, &true);
        extend_instance_ttl(&env);

        BondInitialized {
            issuer,
            total_supply,
            maturity_timestamp,
            coupon_rate_bps,
            coupon_periods_per_year,
            settlement_asset,
        }
        .publish(&env);
    }

    // -----------------------------------------------------------------------
    // Issuer operations
    // -----------------------------------------------------------------------

    /// Mint `amount` bond tokens to `to`. Only callable by the issuer.
    /// `to` must be registered in the KYC Whitelist contract.
    pub fn mint(env: Env, to: Address, amount: i128) {
        let issuer: Address = env
            .storage()
            .instance()
            .get(&DataKey::Issuer)
            .expect("not initialized");
        issuer.require_auth();

        assert!(amount > 0, "amount must be positive");
        extend_instance_ttl(&env);

        // Enforce KYC whitelist
        Self::assert_whitelisted(&env, &to);

        // Burns reduce outstanding supply, but must never restore issuance capacity.
        let lifetime_issued: i128 = env
            .storage()
            .instance()
            .get(&DataKey::LifetimeIssuedSupply)
            .unwrap_or(0);
        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .expect("not initialized");
        assert!(
            lifetime_issued + amount <= total,
            "exceeds total_supply cap"
        );

        // Update balances
        let current: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(to.clone()))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(current + amount));
        extend_balance_ttl(&env, &DataKey::Balance(to.clone()));
        let outstanding: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MintedSupply)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::MintedSupply, &(outstanding + amount));
        env.storage()
            .instance()
            .set(&DataKey::LifetimeIssuedSupply, &(lifetime_issued + amount));

        // Address goes in the topics (indexed) so the dashboard can filter
        // mints by investor without scanning every event.
        Mint { to, amount }.publish(&env);
    }

    /// Refresh the contract instance and a holder's persistent balance entry.
    /// Anyone may call this; the caller pays the storage-rent transaction fee.
    pub fn refresh_balance_ttl(env: Env, owner: Address) -> bool {
        extend_instance_ttl(&env);
        let key = DataKey::Balance(owner);
        let exists = env.storage().persistent().has(&key);
        extend_balance_ttl(&env, &key);
        exists
    }

    /// Returns all immutable bond parameters.
    pub fn get_bond_info(env: Env) -> BondInfo {
        extend_instance_ttl(&env);
        BondInfo {
            issuer: env
                .storage()
                .instance()
                .get(&DataKey::Issuer)
                .expect("not initialized"),
            total_supply: env
                .storage()
                .instance()
                .get(&DataKey::TotalSupply)
                .expect("not initialized"),
            minted_supply: env
                .storage()
                .instance()
                .get(&DataKey::MintedSupply)
                .unwrap_or(0),
            lifetime_issued_supply: env
                .storage()
                .instance()
                .get(&DataKey::LifetimeIssuedSupply)
                .unwrap_or(0),
            maturity_timestamp: env
                .storage()
                .instance()
                .get(&DataKey::MaturityTimestamp)
                .expect("not initialized"),
            coupon_rate_bps: env
                .storage()
                .instance()
                .get(&DataKey::CouponRateBps)
                .expect("not initialized"),
            coupon_periods_per_year: env
                .storage()
                .instance()
                .get(&DataKey::CouponPeriodsPerYear)
                .expect("not initialized"),
            settlement_asset: env
                .storage()
                .instance()
                .get(&DataKey::SettlementAsset)
                .expect("not initialized"),
            isin: env
                .storage()
                .instance()
                .get(&DataKey::Isin)
                .expect("not initialized"),
            whitelist_contract: env
                .storage()
                .instance()
                .get(&DataKey::WhitelistContract)
                .expect("not initialized"),
        }
    }

    pub fn get_coupon_terms(env: Env) -> CouponTerms {
        extend_instance_ttl(&env);
        CouponTerms {
            annual_rate_bps: env
                .storage()
                .instance()
                .get(&DataKey::CouponRateBps)
                .expect("not initialized"),
            periods_per_year: env
                .storage()
                .instance()
                .get(&DataKey::CouponPeriodsPerYear)
                .expect("not initialized"),
            settlement_asset: env
                .storage()
                .instance()
                .get(&DataKey::SettlementAsset)
                .expect("not initialized"),
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn transfer_internal(
        env: &Env,
        from: Address,
        to: Address,
        amount: i128,
        to_muxed_id: Option<u64>,
    ) {
        assert!(amount > 0, "amount must be positive");
        extend_instance_ttl(env);
        Self::assert_whitelisted(env, &from);
        Self::assert_whitelisted(env, &to);

        let from_key = DataKey::Balance(from.clone());
        let to_key = DataKey::Balance(to.clone());
        let from_balance: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
        assert!(from_balance >= amount, "insufficient balance");
        let to_balance: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);

        env.storage()
            .persistent()
            .set(&from_key, &(from_balance - amount));
        env.storage()
            .persistent()
            .set(&to_key, &(to_balance + amount));
        extend_balance_ttl(env, &from_key);
        extend_balance_ttl(env, &to_key);

        Transfer {
            from,
            to,
            amount,
            to_muxed_id,
        }
        .publish(env);
    }

    fn burn_internal(env: &Env, from: Address, amount: i128) {
        assert!(amount > 0, "amount must be positive");
        extend_instance_ttl(env);

        let balance_key = DataKey::Balance(from.clone());
        let from_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        assert!(from_balance >= amount, "insufficient balance");
        env.storage()
            .persistent()
            .set(&balance_key, &(from_balance - amount));
        extend_balance_ttl(env, &balance_key);

        let outstanding: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MintedSupply)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::MintedSupply, &(outstanding - amount));

        Burn { from, amount }.publish(env);
    }

    fn consume_allowance(env: &Env, from: &Address, spender: &Address, amount: i128) {
        assert!(amount > 0, "amount must be positive");
        let key = DataKey::Allowance(from.clone(), spender.clone());
        let current_ledger = env.ledger().sequence();
        let allowance: AllowanceData =
            env.storage()
                .persistent()
                .get(&key)
                .unwrap_or(AllowanceData {
                    amount: 0,
                    live_until_ledger: 0,
                });
        assert!(
            allowance.live_until_ledger >= current_ledger,
            "allowance expired"
        );
        assert!(allowance.amount >= amount, "insufficient allowance");

        env.storage().persistent().set(
            &key,
            &AllowanceData {
                amount: allowance.amount - amount,
                live_until_ledger: allowance.live_until_ledger,
            },
        );
        extend_allowance_ttl(env, &key);
    }

    fn assert_whitelisted(env: &Env, address: &Address) {
        let whitelist_id: Address = env
            .storage()
            .instance()
            .get(&DataKey::WhitelistContract)
            .expect("not initialized");

        let mut args: Vec<Val> = Vec::new(env);
        args.push_back(address.to_val());

        let is_whitelisted: bool =
            env.invoke_contract(&whitelist_id, &symbol_short!("is_wl"), args);

        assert!(is_whitelisted, "address not in KYC whitelist");
    }
}

#[contractimpl]
impl TokenInterface for GreenBondContract {
    fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        extend_instance_ttl(&env);
        let key = DataKey::Allowance(from, spender);
        let current_ledger = env.ledger().sequence();
        let allowance: AllowanceData = match env.storage().persistent().get(&key) {
            Some(allowance) => allowance,
            None => return 0,
        };

        if allowance.live_until_ledger < current_ledger {
            env.storage().persistent().remove(&key);
            return 0;
        }

        extend_allowance_ttl(&env, &key);
        allowance.amount
    }

    fn approve(env: Env, from: Address, spender: Address, amount: i128, live_until_ledger: u32) {
        from.require_auth();
        assert!(amount >= 0, "amount must not be negative");

        let current_ledger = env.ledger().sequence();
        assert!(
            amount == 0 || live_until_ledger >= current_ledger,
            "allowance expiration is in the past"
        );
        assert!(
            amount == 0 || live_until_ledger <= current_ledger.saturating_add(MAX_ENTRY_TTL),
            "allowance expiration exceeds maximum TTL"
        );

        let key = DataKey::Allowance(from.clone(), spender.clone());
        env.storage().persistent().set(
            &key,
            &AllowanceData {
                amount,
                live_until_ledger,
            },
        );
        extend_allowance_ttl(&env, &key);
        Approve {
            from,
            spender,
            amount,
            live_until_ledger,
        }
        .publish(&env);
    }

    fn balance(env: Env, id: Address) -> i128 {
        extend_instance_ttl(&env);
        let key = DataKey::Balance(id);
        extend_balance_ttl(&env, &key);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    fn transfer(env: Env, from: Address, to: MuxedAddress, amount: i128) {
        from.require_auth();
        let to_muxed_id = to.id();
        Self::transfer_internal(&env, from, to.address(), amount, to_muxed_id);
    }

    fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
        spender.require_auth();
        Self::consume_allowance(&env, &from, &spender, amount);
        Self::transfer_internal(&env, from, to, amount, None);
    }

    fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();
        Self::burn_internal(&env, from, amount);
    }

    fn burn_from(env: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();
        Self::consume_allowance(&env, &from, &spender, amount);
        Self::burn_internal(&env, from, amount);
    }

    fn decimals(env: Env) -> u32 {
        extend_instance_ttl(&env);
        env.storage()
            .instance()
            .get(&DataKey::Decimals)
            .expect("not initialized")
    }

    fn name(env: Env) -> String {
        extend_instance_ttl(&env);
        env.storage()
            .instance()
            .get(&DataKey::Name)
            .expect("not initialized")
    }

    fn symbol(env: Env) -> String {
        extend_instance_ttl(&env);
        env.storage()
            .instance()
            .get(&DataKey::TokenSymbol)
            .expect("not initialized")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test;
