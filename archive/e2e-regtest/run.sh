#!/usr/bin/env bash
# Reproduce the Cloakroom joint-key e2e on a fresh Zebra regtest node.
set -euo pipefail
cd "$(dirname "$0")"

docker pull zfnd/zebra:7.0.0-rc.0
(cd zebra && docker compose down >/dev/null 2>&1 || true; docker compose up -d)

echo "waiting for Zebra RPC on 127.0.0.1:18232 ..."
for _ in $(seq 1 60); do
  if curl -s -H 'content-type: application/json' \
       --data '{"jsonrpc":"2.0","id":1,"method":"getblockchaininfo","params":[]}' \
       http://127.0.0.1:18232 | grep -q '"nextblock":"37a5165b"'; then
    break
  fi
  sleep 2
done

cargo build
mkdir -p evidence
./target/debug/e2e-regtest | tee evidence/console.txt
python3 -I scripts/excerpt.py evidence/run.log > evidence/rpc_excerpts.txt
echo "evidence written to evidence/{console.txt,run.log,rpc_excerpts.txt,sweep_tx.hex,tampered_b_only_tx.hex}"
