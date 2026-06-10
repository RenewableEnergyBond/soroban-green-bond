//! Rebond — Coupon & Redemption Contract
//!
//! Handles automated USDC coupon distribution (periodic) and principal
//! redemption at maturity for Rebond green bonds.
//!
//! USDC flows:
//!  - Coupon: the issuer deposits USDC into this contract, then triggers
//!    `pay_coupon`. The contract reads the bondholder list from the Green Bond
//!    contract, calculates each holder's pro-rata share, and executes atomic
//!    multi-recipient USDC transfers.
//!
//!  - Redemption: at maturity, the issuer deposits principal USDC. Holders
//!    call `redeem` to receive their pro-rata principal in exchange for their
//!    bond tokens being burned.
//!
//! NOTE: This module is a work-in-progress scaffold. Full multi-recipient USDC
//! payout logic and token-burn integration will be completed in Tranche 2.
//!
//! Licensed under the MIT License.
//! https://github.com/RenewableEnergyBond/soroban-green-bond

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol, Vec};

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
    /// Address of the USDC token contract on Stellar
    UsdcToken,
    /// Coupon payment schedule: list of (timestamp, rate_bps) tuples
    CouponSchedule,
    /// Index of the next coupon payment to be executed
    NextCouponIndex,
    /// Guards against double-initialisation
    Initialized,
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub struct CouponEntry {
    /// Unix timestamp when this coupon becomes payable
    pub payment_timestamp: u64,
    /// Coupon rate for this period in basis points
    pub rate_bps: u32,
}

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
    /// * `usdc_token`    – Address of the USDC token contract on Stellar mainnet.
    pub fn initialize(
        env: Env,
        issuer: Address,
        bond_contract: Address,
        usdc_token: Address,
    ) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Issuer, &issuer);
        env.storage()
            .instance()
            .set(&DataKey::BondContract, &bond_contract);
        env.storage()
            .instance()
            .set(&DataKey::UsdcToken, &usdc_token);
        env.storage()
            .instance()
            .set(&DataKey::NextCouponIndex, &0_u32);
        env.storage().instance().set(&DataKey::Initialized, &true);

        env.events()
            .publish((Symbol::new(&env, "cr_initialized"),), (&issuer, &bond_contract));
    }

    // -----------------------------------------------------------------------
    // Coupon payments
    // -----------------------------------------------------------------------

    /// Trigger the next scheduled coupon payment to all whitelisted bondholders.
    ///
    /// The issuer must have pre-funded this contract with the required USDC
    /// amount before calling this function. The contract verifies the payment
    /// timestamp has been reached and distributes funds atomically.
    ///
    /// Full multi-recipient USDC payout implementation: Tranche 2.
    pub fn pay_coupon(env: Env, holder_addresses: Vec<Address>) {
        let issuer: Address = env
            .storage()
            .instance()
            .get(&DataKey::Issuer)
            .expect("not initialized");
        issuer.require_auth();

        let _bond_contract: Address = env
            .storage()
            .instance()
            .get(&DataKey::BondContract)
            .expect("not initialized");

        // TODO Tranche 2: for each holder in holder_addresses, read balance
        // from the Green Bond contract, compute pro-rata USDC amount, and
        // execute USDC transfer via the token contract interface.

        env.events().publish(
            (Symbol::new(&env, "coupon_paid"),),
            (holder_addresses.len() as u32,),
        );
    }

    // -----------------------------------------------------------------------
    // Redemption at maturity
    // -----------------------------------------------------------------------

    /// Redeem bond tokens at maturity. The caller must hold bond tokens.
    /// The contract burns the tokens and transfers the pro-rata USDC principal.
    ///
    /// Full redemption implementation: Tranche 2.
    pub fn redeem(env: Env, holder: Address) {
        holder.require_auth();

        let _bond_contract: Address = env
            .storage()
            .instance()
            .get(&DataKey::BondContract)
            .expect("not initialized");

        // TODO Tranche 2: verify maturity timestamp reached, read holder balance
        // from Green Bond contract, calculate USDC redemption amount, burn
        // bond tokens, transfer USDC to holder.

        env.events()
            .publish((Symbol::new(&env, "redeemed"),), (&holder,));
    }

    // -----------------------------------------------------------------------
    // View functions
    // -----------------------------------------------------------------------

    /// Returns the address of the bond contract this module is linked to.
    pub fn get_bond_contract(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::BondContract)
            .expect("not initialized")
    }

    /// Returns the USDC token contract address.
    pub fn get_usdc_token(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::UsdcToken)
            .expect("not initialized")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test;
