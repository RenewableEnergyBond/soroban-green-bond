#!/usr/bin/env bash
# deploy-testnet.sh — Deploy Rebond Soroban contracts to Stellar testnet
#
# Prerequisites:
#   - Stellar CLI v30.0.1+ installed
#   - Rust wasm32v1-none target: rustup target add wasm32v1-none
#   - Testnet account funded: stellar keys generate --network testnet deployer
#
# Usage: ./scripts/deploy-testnet.sh

set -euo pipefail

NETWORK="testnet"
SOURCE="deployer"

echo "==> Building WASM contracts..."
stellar contract build --locked

echo ""
echo "==> Deploying KYC Whitelist contract..."
WHITELIST_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/kyc_whitelist.wasm \
  --source "$SOURCE" \
  --network "$NETWORK")
echo "    KYC Whitelist contract ID: $WHITELIST_ID"

echo ""
echo "==> Deploying Green Bond contract..."
BOND_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/green_bond.wasm \
  --source "$SOURCE" \
  --network "$NETWORK")
echo "    Green Bond contract ID: $BOND_ID"

echo ""
echo "==> Deploying Coupon & Redemption contract..."
COUPON_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/coupon_redemption.wasm \
  --source "$SOURCE" \
  --network "$NETWORK")
echo "    Coupon/Redemption contract ID: $COUPON_ID"

echo ""
echo "==> Summary"
echo "    Network:           $NETWORK"
echo "    Whitelist:         $WHITELIST_ID"
echo "    Green Bond:        $BOND_ID"
echo "    Coupon/Redemption: $COUPON_ID"
echo ""
echo "    Stellar Expert (testnet):"
echo "    https://testnet.steexp.com/contract/$BOND_ID"
echo ""
echo "Next steps:"
echo "  1. Initialize the whitelist:"
echo "     stellar contract invoke --id $WHITELIST_ID --source $SOURCE --network $NETWORK \\"
echo "       -- initialize --admin <YOUR_ISSUER_ADDRESS>"
echo ""
echo "  2. Add an investor to the whitelist:"
echo "     stellar contract invoke --id $WHITELIST_ID --source $SOURCE --network $NETWORK \\"
echo "       -- add --address <INVESTOR_ADDRESS>"
echo ""
echo "  3. Initialize the bond:"
echo "     stellar contract invoke --id $BOND_ID --source $SOURCE --network $NETWORK \\"
echo "       -- initialize \\"
echo "       --config '{\"issuer\":\"<YOUR_ISSUER_ADDRESS>\",\"total_supply\":1000000,\"maturity_timestamp\":1972800000,\"coupon_rate_bps\":500,\"coupon_periods_per_year\":4,\"settlement_asset\":\"<VERIFIED_EURC_SAC_ID>\",\"isin\":\"FRRBD00001\",\"name\":\"Rebond Green Bond 2032\",\"symbol\":\"RGB32\",\"decimals\":0,\"whitelist_contract\":\"$WHITELIST_ID\"}'"
