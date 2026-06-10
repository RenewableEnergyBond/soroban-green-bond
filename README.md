# Rebond — Soroban Green Bond

**Regulated green bond tokenization on Stellar** | MiFID II · EU placement privé · Soroban smart contracts

> Part of the [Rebond](https://rebond.eco) platform — connecting European renewable energy IPPs with institutional investors through on-chain green bond issuance.

---

## Overview

This repository contains the three Soroban smart contracts powering the Rebond green bond lifecycle on Stellar:

| Contract | Description |
|---|---|
| [`green-bond`](contracts/green-bond/) | Core security token — issuance, supply cap, maturity, coupon rate, transfer restrictions |
| [`kyc-whitelist`](contracts/kyc-whitelist/) | On-chain KYC registry — only verified addresses can hold or receive bond tokens |
| [`coupon-redemption`](contracts/coupon-redemption/) | Automated USDC coupon distribution and principal redemption at maturity |

**Network:** Stellar (Soroban)  
**Settlement:** USDC (native on Stellar)  
**Regulatory framework:** MiFID II security tokens, placement privé Art. L.411-2 CMF (France)  
**License:** MIT

---

## Architecture

```
[Issuer (Rebond UI / rebond.eco)]
        │
        ▼
[Node.js Backend / Rebond API]
        │
        ├── Soroban RPC ──► [Green Bond Contract]
        │                        ├── Token issuance (mint)
        │                        ├── Transfer (checks KYC Whitelist)
        │                        └── Bond info (immutable parameters)
        │
        ├── Soroban RPC ──► [KYC Whitelist Contract]
        │                        ├── add / remove investor
        │                        └── is_wl (called on every transfer)
        │
        ├── Soroban RPC ──► [Coupon & Redemption Contract]
        │                        ├── pay_coupon (USDC → all holders)
        │                        └── redeem (USDC → holder, burn tokens)
        │
        └── Horizon API ──► [Stellar Network]
                                 ├── Account management
                                 ├── USDC transfers
                                 └── Event stream → investor dashboard

[Investor] ──► Stellar Wallet (Freighter / Albedo / LOBSTR)
               ──► holds bond tokens
               ──► receives USDC coupons quarterly
               ──► receives USDC principal at maturity
```

---

## Contracts

### Green Bond Contract

The core security token contract. Parameters are immutably stored on-chain at issuance.

```rust
// Initialize a bond
initialize(issuer, total_supply, maturity_timestamp, coupon_rate_bps, isin, whitelist_contract)

// Mint tokens to a whitelisted investor
mint(to, amount)

// Transfer tokens (both parties must be whitelisted)
transfer(from, to, amount)

// Read investor balance
balance(owner) -> i128

// Read all bond parameters
get_bond_info() -> BondInfo
```

**Compliance:** every `mint` and `transfer` call performs a cross-contract check against the KYC Whitelist. Non-whitelisted addresses are rejected at the contract level.

### KYC Whitelist Contract

On-chain registry mapping investor addresses to their KYC verification status.

```rust
// Initialize with admin address (Rebond platform wallet or multi-sig)
initialize(admin)

// Add a KYC-verified investor
add(address)

// Remove an investor (e.g. expired KYC)
remove(address)

// Check whitelist status (called by Green Bond on every transfer)
is_wl(address) -> bool

// Transfer admin to a new address (e.g. multi-sig upgrade)
transfer_admin(new_admin)

// Statistics
whitelist_count() -> u32
```

### Coupon & Redemption Contract *(Tranche 2 — in progress)*

Handles automated USDC distribution to all bondholders.

```rust
// Initialize with bond contract and USDC token addresses
initialize(issuer, bond_contract, usdc_token)

// Distribute coupon USDC to all whitelisted holders (atomic multi-recipient)
pay_coupon(holder_addresses)

// Redeem principal USDC at maturity in exchange for bond tokens
redeem(holder)
```

---

## Development

### Prerequisites

```bash
# Rust + wasm32 target
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown

# Soroban CLI
cargo install --locked soroban-cli
```

### Run tests

```bash
cargo test --all --features testutils
```

### Build WASM

```bash
cargo build --target wasm32-unknown-unknown --release \
  -p green-bond -p kyc-whitelist -p coupon-redemption
```

### Deploy to testnet

```bash
# Fund a testnet account first
soroban keys generate --network testnet deployer

# Then run the deploy script
chmod +x scripts/deploy-testnet.sh
./scripts/deploy-testnet.sh
```

---

## Roadmap

| Milestone | Status | Target |
|---|---|---|
| Green Bond Contract (core) | 🚧 In progress | Tranche 1 |
| KYC Whitelist Contract | 🚧 In progress | Tranche 1 |
| Backend Adapter (Node.js + Horizon + Soroban RPC) | 📋 Planned | Tranche 1 |
| USDC Coupon Distribution (multi-recipient atomic) | 📋 Planned | Tranche 2 |
| Investor Onboarding (Stellar Wallets Kit + DFNS) | 📋 Planned | Tranche 2 |
| Investor Dashboard (rebond.eco/portfolio) | 📋 Planned | Tranche 2 |
| Mainnet Deployment + Certora Audit | 📋 Planned | Tranche 3 |
| First Live Issuance (LANGA International pilot) | 📋 Planned | Tranche 3 |
| Open-Source Release (MIT) | 📋 Planned | Tranche 3 |

---

## Security

This code is being prepared for audit by Certora via the Stellar LaunchKit audit credit (unlocked at Tranche 2 review).

**Do not deploy unaudited contracts to mainnet for production use.**

To report a security vulnerability, contact: security@rebond.eco

---

## License

MIT — see [LICENSE](LICENSE)

---

## About Rebond

[Rebond](https://rebond.eco) is a French fintech platform enabling independent renewable energy producers (IPPs) to issue regulated green bonds as Soroban security tokens on Stellar, connected to institutional investors via MiFID II-licensed placement agents.

- **Website:** https://rebond.eco  
- **Stellar Community Fund:** SCF Build Grant (Integration Track)  
- **Contact:** contact@rebond.eco
