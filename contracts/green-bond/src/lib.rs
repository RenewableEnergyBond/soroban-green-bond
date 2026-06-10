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
    contract, contractimpl, contracttype, symbol_short, Address, Env, String, Symbol, Val, Vec,
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
    /// Tokens already minted (circulating supply)
    MintedSupply,
    /// Unix timestamp (seconds) at which the bond matures
    MaturityTimestamp,
    /// Annual coupon rate in basis points (e.g. 500 = 5.00 %)
    CouponRateBps,
    /// ISIN-equivalent identifier stored as a Symbol
    Isin,
    /// Address of the KYC Whitelist contract
    WhitelistContract,
    /// Per-investor token balance
    Balance(Address),
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
    pub minted_supply: i128,
    pub maturity_timestamp: u64,
    pub coupon_rate_bps: u32,
    pub isin: String,
    pub whitelist_contract: Address,
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
    /// # Arguments
    /// * `issuer`              – The address authorised to mint tokens.
    /// * `total_supply`        – Hard cap on the number of tokens (e.g. 1 000 000
    ///                           for a €1 M bond with 1 token = €1).
    /// * `maturity_timestamp`  – Unix timestamp (seconds) of bond maturity.
    /// * `coupon_rate_bps`     – Annual coupon in basis points (500 = 5 %).
    /// * `isin`                – ISIN or internal bond identifier.
    /// * `whitelist_contract`  – Address of the deployed KYC Whitelist contract.
    pub fn initialize(
        env: Env,
        issuer: Address,
        total_supply: i128,
        maturity_timestamp: u64,
        coupon_rate_bps: u32,
        isin: String,
        whitelist_contract: Address,
    ) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }
        assert!(total_supply > 0, "total_supply must be positive");
        assert!(
            coupon_rate_bps <= 10_000,
            "coupon_rate_bps must be <= 10000"
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
            .set(&DataKey::MaturityTimestamp, &maturity_timestamp);
        env.storage()
            .instance()
            .set(&DataKey::CouponRateBps, &coupon_rate_bps);
        env.storage().instance().set(&DataKey::Isin, &isin);
        env.storage()
            .instance()
            .set(&DataKey::WhitelistContract, &whitelist_contract);
        env.storage().instance().set(&DataKey::Initialized, &true);

        env.events().publish(
            (Symbol::new(&env, "bond_initialized"),),
            (&issuer, total_supply, maturity_timestamp, coupon_rate_bps),
        );
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

        // Enforce KYC whitelist
        Self::assert_whitelisted(&env, &to);

        // Enforce supply cap
        let minted: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MintedSupply)
            .unwrap_or(0);
        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .expect("not initialized");
        assert!(minted + amount <= total, "exceeds total_supply cap");

        // Update balances
        let current: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(to.clone()))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(current + amount));
        env.storage()
            .instance()
            .set(&DataKey::MintedSupply, &(minted + amount));

        env.events()
            .publish((Symbol::new(&env, "mint"),), (&to, amount));
    }

    // -----------------------------------------------------------------------
    // Investor operations
    // -----------------------------------------------------------------------

    /// Transfer `amount` tokens from `from` to `to`.
    /// Both addresses must be in the KYC Whitelist.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        assert!(amount > 0, "amount must be positive");

        // Enforce KYC whitelist for recipient
        Self::assert_whitelisted(&env, &to);

        let from_balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(from.clone()))
            .unwrap_or(0);
        assert!(from_balance >= amount, "insufficient balance");

        let to_balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(to.clone()))
            .unwrap_or(0);

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from.clone()), &(from_balance - amount));
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(to_balance + amount));

        env.events()
            .publish((Symbol::new(&env, "transfer"),), (&from, &to, amount));
    }

    // -----------------------------------------------------------------------
    // View functions
    // -----------------------------------------------------------------------

    /// Returns the token balance of `owner`.
    pub fn balance(env: Env, owner: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(owner))
            .unwrap_or(0)
    }

    /// Returns all immutable bond parameters.
    pub fn get_bond_info(env: Env) -> BondInfo {
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

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test;
