//! Rebond — KYC Whitelist Contract
//!
//! On-chain transfer restriction registry for the Rebond green bond platform.
//! Only addresses registered here by the admin can hold or receive bond tokens,
//! meeting MiFID II qualified-investor compliance requirements (Art. L.411-2 CMF).
//!
//! The admin address is set at initialisation and can be transferred to a
//! multi-sig or DAO governance contract in future upgrades.
//!
//! Licensed under the MIT License.
//! https://github.com/RenewableEnergyBond/soroban-green-bond

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Address authorised to add/remove investors
    Admin,
    /// Guards against double-initialisation
    Initialized,
    /// Per-investor whitelist entry (true = whitelisted)
    Whitelisted(Address),
    /// Running count of whitelisted addresses (informational)
    WhitelistCount,
}

// ---------------------------------------------------------------------------
// Storage TTL (rent) management
// ---------------------------------------------------------------------------
//
// Soroban storage is rented: entries are archived if nobody extends them.
// Whitelist entries gate every bond transfer until the bond matures (2032),
// so they must be bumped on every interaction to avoid being archived.

/// ~1 day of ledgers at a 5-second close time.
const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;
const ENTRY_BUMP_AMOUNT: u32 = 90 * DAY_IN_LEDGERS;
const ENTRY_LIFETIME_THRESHOLD: u32 = ENTRY_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Extend the TTL of the instance storage (admin, count).
fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Extend the TTL of a persistent whitelist entry, if it exists.
fn extend_entry_ttl(env: &Env, key: &DataKey) {
    if env.storage().persistent().has(key) {
        env.storage()
            .persistent()
            .extend_ttl(key, ENTRY_LIFETIME_THRESHOLD, ENTRY_BUMP_AMOUNT);
    }
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct KycWhitelistContract;

#[contractimpl]
impl KycWhitelistContract {
    // -----------------------------------------------------------------------
    // Initialisation
    // -----------------------------------------------------------------------

    /// Initialise the whitelist. Can only be called once.
    ///
    /// # Arguments
    /// * `admin` – The address authorised to manage the whitelist (typically
    ///             the Rebond platform backend wallet or a multi-sig).
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::WhitelistCount, &0_u32);
        env.storage().instance().set(&DataKey::Initialized, &true);
        extend_instance_ttl(&env);

        env.events()
            .publish((Symbol::new(&env, "wl_initialized"),), (&admin,));
    }

    // -----------------------------------------------------------------------
    // Admin operations
    // -----------------------------------------------------------------------

    /// Add `address` to the whitelist. Only callable by the admin.
    /// Emits a `wl_add` event with the registered address.
    pub fn add(env: Env, address: Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();

        let already: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Whitelisted(address.clone()))
            .unwrap_or(false);

        if !already {
            env.storage()
                .persistent()
                .set(&DataKey::Whitelisted(address.clone()), &true);
            extend_entry_ttl(&env, &DataKey::Whitelisted(address.clone()));

            let count: u32 = env
                .storage()
                .instance()
                .get(&DataKey::WhitelistCount)
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::WhitelistCount, &(count + 1));
            extend_instance_ttl(&env);

            // Address indexed as a topic so a per-investor whitelist history
            // can be queried without scanning all events.
            env.events()
                .publish((Symbol::new(&env, "wl_add"), address.clone()), ());
        }
    }

    /// Remove `address` from the whitelist. Only callable by the admin.
    /// Emits a `wl_remove` event.
    pub fn remove(env: Env, address: Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();

        let was_present: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Whitelisted(address.clone()))
            .unwrap_or(false);

        if was_present {
            env.storage()
                .persistent()
                .remove(&DataKey::Whitelisted(address.clone()));

            let count: u32 = env
                .storage()
                .instance()
                .get(&DataKey::WhitelistCount)
                .unwrap_or(0);
            if count > 0 {
                env.storage()
                    .instance()
                    .set(&DataKey::WhitelistCount, &(count - 1));
            }

            env.events()
                .publish((Symbol::new(&env, "wl_remove"), address.clone()), ());
        }
    }

    /// Transfer admin rights to `new_admin`. Only callable by the current admin.
    pub fn transfer_admin(env: Env, new_admin: Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &new_admin);

        env.events()
            .publish((Symbol::new(&env, "admin_transfer"),), (&admin, &new_admin));
    }

    // -----------------------------------------------------------------------
    // View functions (called by the Green Bond contract on every transfer)
    // -----------------------------------------------------------------------

    /// Returns `true` if `address` is registered in the whitelist.
    /// This is the function invoked cross-contract by the Green Bond on every
    /// `mint` and `transfer` call.
    pub fn is_wl(env: Env, address: Address) -> bool {
        let key = DataKey::Whitelisted(address);
        extend_entry_ttl(&env, &key);
        env.storage().persistent().get(&key).unwrap_or(false)
    }

    /// Returns the current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized")
    }

    /// Returns the number of whitelisted addresses.
    pub fn whitelist_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::WhitelistCount)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test;
