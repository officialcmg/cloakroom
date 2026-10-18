#!/usr/bin/env bash
# Regenerates verification harnesses from the CURRENT src/ files (src/ is never modified).
#  - test/generated/CloakroomEscrowStubbed.sol : verbatim copy of src/CloakroomEscrow.sol with
#    (a) the contract renamed and (b) `import {Pallas} from "./Pallas.sol"` re-pointed at
#    PallasStub.sol, whose isOnCurve/isDiscreteLogOf are UNINTERPRETED PREDICATES (keccak low bit).
#    Used ONLY for state-machine / accounting properties in Halmos.
#  - test/generated/PallasSmall{223,4987}.sol : verbatim copies of src/Pallas.sol with the
#    library renamed and P/Q/B/GX/GY replaced by a small prime-order a=0 curve, for exhaustive
#    testing of the exact formula/branch text.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=test/generated
mkdir -p "$OUT"
grep -q 'import {Pallas} from "./Pallas.sol";' src/CloakroomEscrow.sol
sed -e 's#import {Pallas} from "./Pallas.sol";#import {Pallas} from "./PallasStub.sol";#' \
    -e 's/^contract CloakroomEscrow /contract CloakroomEscrowStubbed /' \
    -e 's/^interface IERC20 /interface IERC20Esc /' -e 's/IERC20(token)/IERC20Esc(token)/g' \
    src/CloakroomEscrow.sol > "$OUT/CloakroomEscrowStubbed.sol"
grep -q 'contract CloakroomEscrowStubbed' "$OUT/CloakroomEscrowStubbed.sol"

gen_small() { # name p q b gx gy
  local name=$1 p=$2 q=$3 b=$4 gx=$5 gy=$6
  sed -E -e "s/^library Pallas /library $name /" \
      -e "s/(uint256 internal constant P = )0x[0-9a-fA-F]+;/\1$p;/" \
      -e "s/(uint256 internal constant Q = )0x[0-9a-fA-F]+;/\1$q;/" \
      -e "s/(uint256 internal constant B = )[0-9]+;/\1$b;/" \
      -e "s/(uint256 internal constant GX = )0x[0-9a-fA-F]+;/\1$gx;/" \
      -e "s/(uint256 internal constant GY = )0x[0-9a-fA-F]+;/\1$gy;/" \
      src/Pallas.sol > "$OUT/$name.sol"
  for c in "P = $p;" "Q = $q;" "B = $b;" "GX = $gx;" "GY = $gy;"; do grep -q "$c" "$OUT/$name.sol"; done
}
gen_small PallasSmall223 223 229 5 3 60
gen_small PallasSmall4987 4987 5023 3 1 2
echo "generated: $(ls $OUT)"
