# ADR 0001: Coupon Settlement and Snapshot

- Status: Accepted for PoC
- Date: 2026-11-19
- Scope: Soroban bond terms, coupon snapshot tool, and claim contract

## Context

Stellar's Soroban resource limits make a single transaction that enumerates and pays every bondholder unsuitable. Coupon entitlements must be fixed against a record-date balance snapshot and paid as separate holder claims. The bond is euro-denominated, while settlement must use an on-chain asset whose contract interface is available to Soroban.

## Decisions

### Bond terms

- Coupon rate is stored as annual basis points.
- Coupon frequency is immutable per bond and is represented as `coupon_periods_per_year`: `1` (annual), `2` (semi-annual), or `4` (quarterly).
- The bond stores the Stellar Asset Contract (SAC) address of its settlement asset. Coupon/redemption initialization reads `get_coupon_terms()` from the bond and rejects a different settlement asset.
- The PoC uses EURC as the intended settlement asset for euro-denominated bonds. EURC is available on Stellar and Soroban contracts interact with it through its SAC. Production deployment configuration must use the currently verified EURC SAC contract ID; the contract does not treat an arbitrary token address as proof of Circle issuance.

### Coupon calculation

- The coupon period is `[period_start_timestamp, period_end_timestamp]` in Unix seconds.
- The off-chain snapshot tool calculates each holder's due amount with Actual/365 Fixed:

  `floor(balance_base_units * annual_rate_bps * period_seconds * 10^settlement_decimals / (10^bond_decimals * 10,000 * 31,536,000))`

- The bond token uses `decimals = 0` for the PoC, where one token represents one euro of nominal principal. EURC uses 7 decimal places.
- Frequency controls the intended schedule; actual elapsed seconds determine the accrued amount. No FX oracle is used: principal and coupon settle in the bond's own currency.

### Record date and snapshot

- The issuer selects a record ledger. Holder balances are reconstructed by replaying `mint`, `transfer`, and `burn` events up to and including that ledger, ordered by ledger, transaction index, and event index.
- Transfers after the record ledger do not change the entitlement. Whitelist status is not rechecked when claiming: the right is fixed by token ownership at the record ledger.
- The operator supplies the outstanding supply captured for the record ledger. `coupon-snapshot` rejects a replayed balance total that differs from this value.
- The issuer publishes the normalized event export, the record ledger and captured outstanding supply, generator inputs, generated claims, and Merkle root. The root commits to XDR `CouponLeaf { coupon_id, holder, amount }` values using a `0x00` leaf domain byte and `0x01` internal-node domain byte; child hashes are sorted lexicographically before hashing.
- The Merkle proof is capped at 32 nodes. The contract does not enumerate holders or calculate balances from a supplied complete holder list.

### Rounding and funds

- Each holder's amount is rounded down to the smallest settlement-asset unit (stroop for EURC).
- The generator reports the aggregate rounding remainder separately. That remainder is not included in the Merkle claims or campaign funding and remains with the issuer.
- The issuer deposits the sum of claimable leaf amounts before campaign creation. The contract verifies available token balance less amounts reserved by other campaigns.
- A claim requires holder authorization, can be submitted only once per holder/campaign, and transfers only the committed amount.
- A bounded claim window is retained for the PoC because duplicate-claim markers use persistent storage with finite TTL. The deadline must fit under the contract storage TTL. After it closes, the issuer may call `close_coupon`; unclaimed claimable funds are returned to the issuer. This return-of-unclaimed-funds rule is a PoC policy and must be reconciled with final bond documents before production.

### Principal redemption

- Redemption is only at maturity for the PoC. Early redemption is deferred.
- The `redeem` entry point remains disabled until its contractual amount, deadline, settlement, and failure rules are specified.
- Future implementation should use a holder-scoped, expiring SEP-41 allowance and `burn_from` for the coupon contract; no unlimited allowance is permitted.

## Implementation

- Bond configuration stores annual rate, periods per year, and settlement SAC address.
- Coupon initialization validates that its settlement SAC matches the linked bond's coupon terms.
- `coupon-snapshot` is an optional Rust CLI (`--features coupon-tools`) that accepts normalized event JSONL and emits a reproducible snapshot/campaign artifact. Its root hashing calls the same exported Rust functions as the contract.
- CI runs the tool's unit tests separately so the optional JSON dependency is not included in release WASM builds.

Example normalized JSONL event:

```json
{"ledger":12345,"transaction_index":0,"event_index":1,"kind":"transfer","from":"G...","to":"G...","amount":"2500"}
```

## Consequences and limitations

- Changing `BondConfig` changes the contract interface; existing testnet deployments are historical and must be redeployed. Their IDs must not be used for resubmission.
- The snapshot export remains an issuer/operator responsibility. Merkle verification proves inclusion, not that the source event export or selected record ledger is correct.
- The tool requires the caller to obtain the record-ledger outstanding supply, settlement SAC ID, bond coupon terms, and settlement decimals from verified chain state/configuration.
- EURC SAC provenance must be verified during deployment/configuration; symbol or name alone is not cryptographic proof of issuer.
- Mainnet deployment requires an independent review of the snapshot procedure, root generation, claim-window policy, keeper/TTL operations, and legal treatment of unclaimed coupons.
