#!/usr/bin/env bash
# deploy-testnet.sh — Deploy Rebond Soroban contracts to Stellar testnet
#
# Prerequisites:
#   - soroban CLI installed (cargo install --locked soroban-cli)
#   - Rust wasm32 target: rustup target add wasm32-unknown-unknown
#   - Testnet account funded: soroban keys generate --network testnet deployer
#
# Usage: ./scripts/deploy-testnet.sh

set -euo pipefail

NETWORK="testnet"
SOURCE="deployer"

echo "==> Building WASM contracts..."
cargo build --target wasm32-unknown-unknown --release \
  -p kyc-whitelist -p green-bond -p coupon-redemption

echo ""
echo "==> Deploying KYC Whitelist contract..."
WHITELIST_ID=$(soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/kyc_whitelist.wasm \
  --source "$SOURCE" \
  --network "$NETWORK")
echo "    KYC Whitelist contract ID: $WHITELIST_ID"

echo ""
echo "==> Deploying Green Bond contract..."
BOND_ID=$(soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/green_bond.wasm \
  --source "$SOURCE" \
  --network "$NETWORK")
echo "    Green Bond contract ID: $BOND_ID"

echo ""
echo "==> Deploying Coupon & Redemption contract..."
COUPON_ID=$(soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/coupon_redemption.wasm \
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
echo "     soroban contract invoke --id $WHITELIST_ID --source $SOURCE --network $NETWORK \\"
echo "       -- initialize --admin <YOUR_ISSUER_ADDRESS>"
echo ""
echo "  2. Add an investor to the whitelist:"
echo "     soroban contract invoke --id $WHITELIST_ID --source $SOURCE --network $NETWORK \\"
echo "       -- add --address <INVESTOR_ADDRESS>"
echo ""
echo "  3. Initialize the bond:"
echo "     soroban contract invoke --id $BOND_ID --source $SOURCE --network $NETWORK \\"
echo "       -- initialize \\"
echo "       --issuer <YOUR_ISSUER_ADDRESS> \\"
echo "       --total_supply 1000000 \\"
echo "       --maturity_timestamp 1972800000 \\"
echo "       --coupon_rate_bps 500 \\"
echo "       --isin FRRBD00001 \\"
echo "       --whitelist_contract $WHITELIST_ID"
