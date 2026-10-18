#!/usr/bin/env bash
# Real per-transaction gas (receipt gasUsed, including the 21000 intrinsic and calldata cost)
# for every successful escrow call, measured on a throwaway anvil chain. Each call is its own
# transaction, so storage access costs are those of a real deployment (cold slots).
# Usage: audit/measure_gas.sh   (writes audit/logs/gas_receipts.md)
set -euo pipefail
cd "$(dirname "$0")/.."
PORT=8599
RPC=http://127.0.0.1:$PORT
# anvil's well-known development keys (accounts 0..2)
MK=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
PK=0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d
RK=0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a
MAKER=$(cast wallet address $MK); PAYOUT=$(cast wallet address $PK); RELAYER=$(cast wallet address $RK)
A_SEC=0x195661e2e4789b7f57137b2e777736d79720270638d12e4882fd95e52a73d6b3
AX=0x17a182b1eb9594581ecaad0fcb61546f4c366cc57b9575ddde22ca56963c304c
AY=0x19b252972f768cc5708b251baca77a9afd02dddd9531510aafb13c4824d7b61f
B_SEC=0x2b8bbfabc504814096b330ffba3d828335e8db236f79aa7fded4d77640369e2a
BX=0x183a6db8e5f566dfd863f5c0c0b846ecfb01d519efbd2259d829319dfd780e95
BY=0x0b8b2c14d8675f49fedab2ee56d17c8547d6627cc8f83b2988e88fad65cb5972
AMT=3500000000; FEE=1000000; W=43200

anvil --port $PORT --chain-id 84532 --silent & APID=$!
trap 'kill $APID 2>/dev/null' EXIT
for _ in $(seq 50); do cast chain-id --rpc-url $RPC >/dev/null 2>&1 && break; sleep 0.2; done

deploy() { forge create --rpc-url $RPC --private-key $MK --broadcast --json "$1" | jq -r .deployedTo; }
USDC=$(deploy src/MockUSDC.sol:MockUSDC)
ESC=$(deploy src/CloakroomEscrow.sol:CloakroomEscrow)
OUT=audit/logs/gas_receipts.md
gas() { # label key sig args...  -> appends "label | gasUsed"
  local label=$1 key=$2; shift 2
  local g; g=$(cast send --rpc-url $RPC --private-key "$key" --json "$ESC" "$@" | jq -r .gasUsed)
  printf '| %s | %d |\n' "$label" "$g" >> $OUT
}
cast send -q --rpc-url $RPC --private-key $MK $USDC "mint(address,uint256)" $MAKER $((2*AMT))
cast send -q --rpc-url $RPC --private-key $MK $USDC "approve(address,uint256)" $ESC $((2*AMT))

{ echo "# Escrow gas per transaction (receipt gasUsed)"; echo;
  echo "anvil $(anvil --version | head -1 | awk '{print $3}'), chain id 84532, solc 0.8.30 via_ir, optimizer 200. Measured $(date -u +%F)."; echo;
  echo "| Transaction | gasUsed |"; echo "|---|---|"; } > $OUT
NOW=$(cast block latest --rpc-url $RPC -f timestamp)
T0=$((NOW+3600)); T1=$((NOW+10800)); T2=$((NOW+18000))
OPEN="open(bytes32,address,address,uint256,uint256,uint256,uint256,uint256,uint256,uint64,uint64,uint64,uint64)"
S1=$(cast format-bytes32-string s1); S2=$(cast format-bytes32-string s2)

gas "open (2 on-curve checks, transferFrom, balance-delta check)" $MK "$OPEN" $S1 $PAYOUT $USDC $AMT $FEE $AX $AY $BX $BY $T0 $T1 $T2 $W
ID=$(cast call --rpc-url $RPC $ESC "swapId(address,bytes32)(bytes32)" $MAKER $S1)
gas "setReady" $MK "setReady(bytes32)" $ID
C=$(cast call --rpc-url $RPC $ESC "commitmentOf(bytes32,uint256)(bytes32)" $ID $A_SEC)
D=$(cast call --rpc-url $RPC $ESC "commitDigest(bytes32,bytes32,address)(bytes32)" $ID $C $RELAYER)
SIG=$(cast wallet sign --no-hash --private-key $PK $D)
R=0x${SIG:2:64}; S=0x${SIG:66:64}; V=$((16#${SIG:130:2}))
gas "commitClaim (ecrecover, sent by relayer)" $RK "commitClaim(bytes32,bytes32,address,uint8,bytes32,bytes32)" $ID $C $RELAYER $V $R $S
gas "claim (scalar check a·G = A, credits payout and relayer)" $RK "claim(bytes32,uint256)" $ID $A_SEC
gas "withdraw (payout)" $RK "withdraw(address,address)" $USDC $PAYOUT
gas "withdraw (relayer)" $RK "withdraw(address,address)" $USDC $RELAYER

gas "open (second swap, warm token balance)" $MK "$OPEN" $S2 $PAYOUT $USDC $AMT $FEE $AX $AY $BX $BY $T0 $T1 $T2 $W
ID2=$(cast call --rpc-url $RPC $ESC "swapId(address,bytes32)(bytes32)" $MAKER $S2)
cast rpc --rpc-url $RPC evm_setNextBlockTimestamp $T1 >/dev/null
gas "refund at t1 (scalar check b·G = B, credits maker)" $MK "refund(bytes32,uint256)" $ID2 $B_SEC
gas "withdraw (maker)" $MK "withdraw(address,address)" $USDC $MAKER
[ "$(cast call --rpc-url $RPC $USDC 'balanceOf(address)(uint256)' $PAYOUT | awk '{print $1}')" = $((AMT-FEE)) ] && echo "check: payout received amount - fee" >> $OUT
cat $OUT
