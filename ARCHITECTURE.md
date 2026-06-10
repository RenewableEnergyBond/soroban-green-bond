# Technical Architecture — Rebond Green Bond Tokenization on Stellar

> **Project:** Rebond — compliant green bond tokenization for European renewable energy IPPs
> **Network:** Stellar (Soroban smart contracts)
> **Status:** Contracts deployed and initialized on **Stellar testnet** (June 2026) — see [Testnet Deployments](#7-testnet-deployments)
> **Repository:** https://github.com/RenewableEnergyBond/soroban-green-bond

This document follows the C4 model: system context (L1), containers (L2), components (L3), plus runtime flows and Stellar integration points.

---

## 1. System Context (C4 Level 1)

```mermaid
graph TB
    subgraph Actors
        ISSUER["Issuer<br/>(Renewable energy IPP,<br/>e.g. LANGA International)"]
        INVESTOR["Qualified Investor<br/>(MiFID II professional client)"]
        AGENT["Distribution Partner<br/>(Black Manta, Brickken —<br/>MiFID II placement agents)"]
    end

    REBOND["Rebond Platform<br/>(rebond.eco)<br/>Issuance, KYC workflow,<br/>project finance monitoring"]

    STELLAR["Stellar Network<br/>Soroban contracts +<br/>native USDC settlement"]

    ISSUER -->|"structures bond,<br/>triggers issuance"| REBOND
    AGENT -->|"onboards investors,<br/>KYC/KYB dossiers"| REBOND
    INVESTOR -->|"subscribes, holds tokens,<br/>receives USDC coupons"| STELLAR
    REBOND -->|"deploys contracts, mints tokens,<br/>manages whitelist, pays coupons"| STELLAR
    INVESTOR -.->|"views holdings &<br/>coupon history"| REBOND
```

**Problem solved:** mid-market European IPPs (€5–30M green debt deals) are excluded from traditional bond issuance due to fixed costs (€150–300K in arranging, listing and paying-agent fees per deal). Rebond replaces this with Soroban contracts and native USDC settlement at near-zero marginal cost.

**Regulatory frame:** French *placement privé* (Art. L.411-2 CMF), MiFID II security tokens, qualified investors only — enforced **on-chain** by the KYC whitelist contract.

---

## 2. Containers (C4 Level 2)

```mermaid
graph TB
    subgraph "Rebond Platform (existing, live at rebond.eco)"
        UI["Web Frontend<br/>Vue.js"]
        API["Backend API<br/>Node.js"]
        PFOP["Project Finance<br/>Operating Platform<br/>(CFADS, DSCR, LLCR,<br/>covenant monitoring)"]
        KYCW["KYC/KYB Workflow<br/>(off-chain verification)"]
    end

    subgraph "Stellar Integration Layer (this grant)"
        ADAPTER["Stellar Adapter<br/>Node.js module<br/>Horizon API + Soroban RPC"]
        DASH["Investor Dashboard<br/>rebond.eco/portfolio<br/>(on-chain data, read-only)"]
    end

    subgraph "Stellar Network"
        GB["Green Bond<br/>Contract (Soroban)"]
        WL["KYC Whitelist<br/>Contract (Soroban)"]
        CR["Coupon/Redemption<br/>Contract (Soroban)"]
        USDC["USDC<br/>(Circle, native asset)"]
        HORIZON["Horizon API"]
        RPC["Soroban RPC"]
    end

    UI --> API
    API --> PFOP
    API --> KYCW
    API --> ADAPTER
    ADAPTER -->|"contract deploy/invoke"| RPC
    ADAPTER -->|"accounts, payments,<br/>tx history"| HORIZON
    RPC --> GB
    RPC --> WL
    RPC --> CR
    CR -->|"transfers"| USDC
    GB -->|"is_wl() cross-contract call"| WL
    DASH -->|"read-only queries"| RPC
    DASH --> HORIZON
```

| Container | Technology | Status | Grant scope |
|---|---|---|---|
| Web Frontend | Vue.js | Live (multi-chain: ERC-1400, ERC-3643/T-REX) | — |
| Backend API | Node.js | Live | — |
| Project Finance Operating Platform | Node.js | Live at rebond.eco | — |
| KYC/KYB Workflow | Node.js + provider | Live | — |
| **Stellar Adapter** | Node.js (Horizon + Soroban RPC) | Planned (Tranche 1) | ✅ |
| **Green Bond Contract** | Rust / Soroban SDK 22 | **Deployed on testnet** | ✅ |
| **KYC Whitelist Contract** | Rust / Soroban SDK 22 | **Deployed on testnet** | ✅ |
| **Coupon/Redemption Contract** | Rust / Soroban SDK 22 | **Deployed on testnet** (scaffold; full logic in Tranche 2) | ✅ |
| **Investor Dashboard** | Vue.js | Planned (Tranche 2) | ✅ |

---

## 3. Components — Soroban Contracts (C4 Level 3)

### 3.1 Green Bond Contract ([contracts/green-bond](contracts/green-bond))

Core MiFID II security token. All bond parameters are immutably stored on-chain at initialization.

| Function | Auth | Description |
|---|---|---|
| `initialize(issuer, total_supply, maturity_timestamp, coupon_rate_bps, isin, whitelist_contract)` | once | Stores bond terms; links the KYC whitelist contract |
| `mint(to, amount)` | issuer | Issues tokens up to `total_supply` cap; recipient must be whitelisted |
| `transfer(from, to, amount)` | holder | KYC-enforced transfer — **both** parties must be whitelisted |
| `balance(owner)` | read | Token balance |
| `get_bond_info()` | read | Full bond terms (issuer, supply, maturity, coupon, ISIN, whitelist) |

**Compliance enforcement:** every `mint` and `transfer` performs a **cross-contract call** to the whitelist contract (`is_wl(address)`). A non-whitelisted address can never hold or receive tokens — compliance is a contract-level invariant, not a backend policy.

### 3.2 KYC Whitelist Contract ([contracts/kyc-whitelist](contracts/kyc-whitelist))

On-chain registry of MiFID II-verified investor addresses. Fed by Rebond's off-chain KYC/KYB workflow via the Stellar Adapter.

| Function | Auth | Description |
|---|---|---|
| `initialize(admin)` | once | Sets registry admin |
| `add(address)` / `remove(address)` | admin | Whitelist management |
| `is_wl(address) → bool` | read | Called cross-contract by the Green Bond contract |
| `transfer_admin(new_admin)` | admin | Admin rotation |
| `whitelist_count()` | read | Number of verified addresses |

### 3.3 Coupon/Redemption Contract ([contracts/coupon-redemption](contracts/coupon-redemption))

Automates the bond lifecycle payments in **native USDC**. Deployed as a scaffold; full payout logic is Tranche 2 scope.

| Function | Auth | Description |
|---|---|---|
| `initialize(issuer, bond_contract, usdc_token)` | once | Links bond contract and USDC asset |
| `pay_coupon(holders)` | issuer | Pro-rata USDC coupon distribution to all whitelisted holders (Tranche 2) |
| `redeem(holder)` | issuer | Principal redemption at maturity in USDC (Tranche 2) |

---

## 4. Runtime Flows

### 4.1 Bond issuance

```mermaid
sequenceDiagram
    participant I as Issuer (Rebond UI)
    participant B as Backend + Stellar Adapter
    participant R as Soroban RPC
    participant GB as Green Bond Contract
    participant WL as Whitelist Contract

    I->>B: Create bond (terms validated by PFOP)
    B->>R: deploy + initialize(GB)
    R->>GB: store terms (supply, maturity, coupon, ISIN)
    B->>R: invoke WL.add(investor) for each KYC-passed investor
    I->>B: Mint to investor
    B->>R: invoke GB.mint(investor, amount)
    GB->>WL: is_wl(investor)?
    WL-->>GB: true
    GB-->>B: minted (event emitted)
```

### 4.2 Coupon payment (Tranche 2)

```mermaid
sequenceDiagram
    participant I as Issuer
    participant B as Backend + Stellar Adapter
    participant CR as Coupon Contract
    participant GB as Green Bond Contract
    participant U as USDC

    I->>B: Trigger coupon date
    B->>CR: pay_coupon(holders)
    loop each holder
        CR->>GB: balance(holder)
        CR->>U: transfer(issuer → holder, pro-rata USDC)
    end
    CR-->>B: coupon_paid event (indexed, auditable)
```

### 4.3 Investor view

Investor dashboard (rebond.eco/portfolio) reads **directly from the chain** (Soroban RPC simulation calls + Horizon transaction history) — holdings, upcoming coupons, payment history, explorer links. No trust in Rebond's database required for bond data.

---

## 5. Stellar Integration Points

| Stellar component | Usage |
|---|---|
| **Soroban smart contracts** (Rust, SDK 22) | 3 contracts: security token, compliance registry, lifecycle payments |
| **Native USDC** (Circle) | Coupon and principal settlement — replaces paying-agent banking rails |
| **Soroban RPC** | Contract deployment, invocations, read-only simulations (dashboard) |
| **Horizon API** | Account management, USDC payment submission, transaction history |
| **Stellar Wallets Kit** | Investor wallet connection on the dashboard (Tranche 2) |
| **Events + indexing** | All state changes emit events (`bond_initialized`, `wl_initialized`, `coupon_paid`, …) for audit trail and explorer visibility |

**Why Stellar:** ~$0.00001 per transaction and 5s finality make per-holder coupon distribution economically viable at any deal size; native USDC removes banking intermediaries from settlement; the network is regulatory-neutral for EU issuers (bond tokens are MiFID II financial instruments, out of MiCA scope).

---

## 6. Security & Compliance Model

- **On-chain enforcement:** transfer restrictions live in the contract, not the backend. The whitelist invariant holds even if Rebond's platform is offline or compromised.
- **Role separation:** issuer (mint, coupon triggers), whitelist admin (KYC registry), holders (transfers). Admin rotation supported.
- **Supply cap:** `total_supply` immutable after initialization; `mint` enforces the cap (overflow-checked arithmetic, `overflow-checks = true` in release profile).
- **Auditability:** every lifecycle action emits an event; full history reconstructable from chain data alone.
- **Audit plan:** external audit of all three contracts before mainnet (Tranche 3 gate), via Stellar LaunchKit — **not** funded by the grant.
- **CI:** every push runs the full test suite (15 unit tests), `clippy --all-targets -D warnings`, `rustfmt`, and a release WASM build ([.github/workflows/ci.yml](.github/workflows/ci.yml)).

---

## 7. Testnet Deployments

Deployed and initialized on Stellar testnet, **10 June 2026** (deployer `GBX3MIAPKVCMK5BJB4FQTADGYMDW7OBOTZXAME3WR4VL4FMADWGTVZO2`):

| Contract | Contract ID |
|---|---|
| KYC Whitelist | [`CAZQL5DAN3MYX5QEIS3EVDEBN2S5AVYJE23EYBTW4L7C5XMJAOIXKL3J`](https://stellar.expert/explorer/testnet/contract/CAZQL5DAN3MYX5QEIS3EVDEBN2S5AVYJE23EYBTW4L7C5XMJAOIXKL3J) |
| Green Bond | [`CB53OFE56JSBHHM4R7J4MU32LJL5F6OG5V7JRAXPL72MN3U44RZBEGS7`](https://stellar.expert/explorer/testnet/contract/CB53OFE56JSBHHM4R7J4MU32LJL5F6OG5V7JRAXPL72MN3U44RZBEGS7) |
| Coupon/Redemption | [`CCN4YGOLXHQCWE6YTH4X5Q76YBHDHZQQPWCZ2Y5HTVRKIOERFNOEOQM7`](https://stellar.expert/explorer/testnet/contract/CCN4YGOLXHQCWE6YTH4X5Q76YBHDHZQQPWCZ2Y5HTVRKIOERFNOEOQM7) |

Live bond on testnet: ISIN-equivalent `FRRBD00001`, 1,000,000 tokens (1 token = €1), 5.00% coupon (500 bps), KYC whitelist enforced on every transfer.

---

## 8. Delivery Roadmap (SCF Build tranches)

| Tranche | Scope | Key architecture milestones |
|---|---|---|
| **T1 — MVP** (M1-2) | Green Bond contract, KYC Whitelist, Stellar Adapter | Contracts hardened + full test coverage; backend can deploy/initialize/mint from Rebond UI |
| **T2 — Testnet** (M3-4) | USDC coupon/redemption logic, investor onboarding, dashboard | End-to-end testnet demo: issuance → whitelist → coupon → redemption, with explorer links |
| **T3 — Mainnet** (M5-6) | Audit remediation, mainnet deploy, first live issuance | First real bond (~€1M, LANGA International), first on-chain coupon, open-source release |
