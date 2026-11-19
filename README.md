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
| [`coupon-redemption`](contracts/coupon-redemption/) | Funded Merkle coupon claims in the bond's settlement asset; principal redemption remains disabled |

**Network:** Stellar (Soroban)  
**Settlement:** same asset as the bond (EURC SAC for euro-denominated PoC bonds)
**Regulatory framework:** MiFID II security tokens, placement privé Art. L.411-2 CMF (France)  
**License:** MIT

📐 **[Technical Architecture →](ARCHITECTURE.md)** — C4 diagrams, runtime flows, Stellar integration points, security model.

---

## Testnet Deployments

> Historical deployment on Stellar testnet — 10 June 2026. These IDs predate the SDK 29 upgrade, lifetime-supply fix, BondConfig initializer, and full SEP-41 interface; redeploy before resubmission.
> Deployer: `GBX3MIAPKVCMK5BJB4FQTADGYMDW7OBOTZXAME3WR4VL4FMADWGTVZO2`

| Contract | Contract ID | Stellar Expert |
|---|---|---|
| KYC Whitelist | `CAZQL5DAN3MYX5QEIS3EVDEBN2S5AVYJE23EYBTW4L7C5XMJAOIXKL3J` | [View on testnet](https://stellar.expert/explorer/testnet/contract/CAZQL5DAN3MYX5QEIS3EVDEBN2S5AVYJE23EYBTW4L7C5XMJAOIXKL3J) |
| Green Bond | `CB53OFE56JSBHHM4R7J4MU32LJL5F6OG5V7JRAXPL72MN3U44RZBEGS7` | [View on testnet](https://stellar.expert/explorer/testnet/contract/CB53OFE56JSBHHM4R7J4MU32LJL5F6OG5V7JRAXPL72MN3U44RZBEGS7) |
| Coupon/Redemption | `CCN4YGOLXHQCWE6YTH4X5Q76YBHDHZQQPWCZ2Y5HTVRKIOERFNOEOQM7` | [View on testnet](https://stellar.expert/explorer/testnet/contract/CCN4YGOLXHQCWE6YTH4X5Q76YBHDHZQQPWCZ2Y5HTVRKIOERFNOEOQM7) |

**Bond parameters (live on testnet):**
- ISIN: `FRRBD00001`
- Total supply: 1,000,000 tokens (1 token = €1 → €1M bond)
- Coupon rate: 5.00% (500 bps)
- Maturity: 2032
- KYC Whitelist: enforced at contract level on every transfer

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
        │                        ├── create_coupon (funded Merkle campaign)
        │                        ├── claim_coupon (settlement asset → holder, once)
        │                        └── redeem (disabled pending terms)
        │
        └── Horizon API ──► [Stellar Network]
                                 ├── Account management
                                 ├── settlement-asset transfers through the SAC
                                 └── Event stream → investor dashboard

[Investor] ──► Stellar Wallet (Freighter / Albedo / LOBSTR)
               ──► holds bond tokens
               ──► claims settlement-asset coupons
               ──► principal redemption at maturity (disabled in this PoC)
```

---

## Contracts

### Green Bond Contract

The core security token contract. Parameters are immutably stored on-chain at issuance.

```rust
// Initialize once with immutable bond terms and token metadata
initialize(BondConfig { issuer, total_supply, maturity_timestamp, coupon_rate_bps,
                        coupon_periods_per_year, settlement_asset, isin, name,
                        symbol, decimals, whitelist_contract })

// Mint tokens to a whitelisted investor
mint(to, amount)

// SEP-41 metadata, balances, direct and delegated transfers/burns
name() / symbol() / decimals()
balance(owner) -> i128
allowance(holder, spender) -> i128
approve(holder, spender, amount, live_until_ledger)
transfer(from, muxed_to, amount) // both addresses must be whitelisted
transfer_from(spender, from, to, amount) // consumes allowance; KYC enforced
burn(holder, amount)
burn_from(spender, holder, amount) // consumes allowance

// Read all bond parameters
get_bond_info() -> BondInfo
```

**Supply invariant:** `lifetime_issued_supply` never decreases and enforces the immutable cap; `minted_supply` is outstanding supply and decreases on burn. Burning can never restore issuance capacity.

**Compliance:** mint and every direct/delegated transfer check both addresses against the KYC Whitelist. A holder can burn even after removal from the whitelist, so KYC expiry does not strand a redemption.

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

### Coupon & Redemption Contract *(Merkle claims implemented; redemption scaffold)*

Coupon campaigns use per-holder Merkle claims, not an atomic payment to an admin-supplied holder list. The coupon contract reads the immutable annual rate, periods per year, and settlement SAC from the linked bond and rejects a different asset. `redeem` remains disabled until its contractual maturity and settlement rules are finalized.

```rust
// Initialize with the bond contract and its verified settlement SAC
initialize(issuer, bond_contract, settlement_sac)

// Register a funded campaign for a published record-ledger snapshot and coupon period
create_coupon(record_ledger, period_start, period_end, claim_deadline, merkle_root, total_amount)

// Each holder claims once with an entitlement proof
claim_coupon(campaign_id, holder, amount, proof)

// Return unclaimed campaign remainder to issuer after claim deadline
close_coupon(campaign_id)

// Principal redemption remains disabled pending terms
redeem(holder)
```

Leaves are XDR-encoded `CouponLeaf { coupon_id, holder, amount }` values hashed as `SHA256(0x00 || leaf_xdr)`; internal nodes are `SHA256(0x01 || sorted(left, right))`. Proof depth is capped at 32. The issuer publishes the record-ledger export and reproducible Merkle root. Coupon amounts use Actual/365 Fixed and round down to the settlement asset's smallest unit; the aggregate rounding remainder is excluded from claims and remains with the issuer. Delisted holders with a positive balance at the record ledger remain eligible. The bounded claim period and issuer return of unclaimed amounts are PoC policies. See [ADR 0001](docs/adr/0001-coupon-settlement-and-snapshot.md).

Generate the campaign artifact from normalized, ordered Soroban events (JSONL) and the outstanding supply captured on-chain at the record ledger:

```bash
cargo run -p coupon-redemption --features coupon-tools --bin coupon-snapshot -- \
        --events events.jsonl --record-ledger 12345 --expected-outstanding 1000000 \
        --coupon-id 0 --period-start 1792224000 --period-end 1800000000 \
        --annual-rate-bps 500 --periods-per-year 4 --bond-decimals 0 \
        --settlement-asset <VERIFIED_EURC_SAC_ID> --settlement-decimals 7 \
        --output coupon-0.json
```

---

## Development

### Prerequisites

```bash
# Rust + Soroban WASM target
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32v1-none

# Stellar CLI v30.0.1+
curl -fsSL https://github.com/stellar/stellar-cli/raw/main/install.sh | sh
```

### Run tests

```bash
cargo test --all --features testutils
cargo test -p coupon-redemption --features coupon-tools
```

### Build WASM

```bash
stellar contract build --locked
```

### Deploy to testnet

```bash
# Fund a testnet account first
stellar keys generate --network testnet deployer

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
| Bond-currency coupon claims (record-date snapshot + Merkle proofs) | 🚧 Initial contract implementation | Tranche 2 |
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
